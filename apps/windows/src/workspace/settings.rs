use super::*;
use crate::theme::Palette;
use gpui_component::input::{EditorState, InputState};
use singboard_core::{settings::Settings, tools::Values};
use std::collections::{HashMap, HashSet};

pub(super) struct DnsRow {
    pub(super) record_type: String,
    pub(super) name: String,
    pub(super) ttl: u32,
    pub(super) value: String,
    pub(super) geo: String,
}

pub(super) struct DnsView {
    pub(super) source_label: String,
    pub(super) records: Vec<DnsRow>,
}

pub(super) struct PanelUpdate {
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    shown: bool,
    key_action: usize,
    keyboard: bool,
    info: singboard_core::update::Check,
    download: Option<(
        std::sync::Arc<std::sync::atomic::AtomicU64>,
        tokio::task::AbortHandle,
    )>,
    package: Option<(std::path::PathBuf, String)>,
    error: String,
}

impl PanelUpdate {
    pub(super) fn new(info: singboard_core::update::Check, focus: FocusHandle) -> Self {
        Self {
            info,
            focus,
            previous_focus: None,
            shown: false,
            key_action: 1,
            keyboard: false,
            download: None,
            package: None,
            error: String::new(),
        }
    }
}

pub(super) enum SettingsDialog {
    ConfirmStop,
    ConfirmUninstall {
        running: bool,
    },
    ConfirmDeleteEndpoint {
        id: String,
        name: String,
        next_name: String,
    },
    GithubToken {
        input: Entity<InputState>,
        reveal: bool,
        error: String,
    },
    ConfirmCoreUpdate {
        summary: String,
        target: String,
        ticket: String,
        values: Values,
        busy: bool,
        error: String,
    },
}

#[derive(Default)]
pub(super) struct SettingsLayout {
    wallpaper_undo: Option<(std::time::Instant, tempfile::TempDir)>,
    mapping_undo: Option<(std::time::Instant, String, String)>,
    pub(super) panel_update: Option<PanelUpdate>,
    pub(super) open: HashSet<&'static str>,
    pub(super) fields: HashMap<&'static str, (Entity<InputState>, String)>,
    pub(super) dns_record: String,
    pub(super) dns_server: String,
    pub(super) dns_history_open: bool,
    pub(super) dns_view: Option<DnsView>,
    pub(super) tint_bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    pub(super) tint_drag: Option<Pixels>,
    pub(super) tint_focus: Option<FocusHandle>,
    pub(super) font_bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    pub(super) font_drag: Option<Pixels>,
    pub(super) font_focus: Option<FocusHandle>,
    pub(super) font_draft: Option<f32>,
    pub(super) font_anchor_y: Option<Pixels>,
    pub(super) font_preview_alpha: f32,
    pub(super) font_preview_tick: Option<std::time::Instant>,
    pub(super) font_preview_last: f32,
    pub(super) results: HashMap<&'static str, String>,
    pub(super) pending: HashSet<&'static str>,
    pub(super) service_action: Option<&'static str>,
    pub(super) api_edit: Option<bool>,
    pub(super) protocol: Option<String>,
    pub(super) service_protocol: Option<String>,
    pub(super) service_ca_editor: Option<(Entity<EditorState>, String)>,
    pub(super) dialog: Option<SettingsDialog>,
}

impl SettingsLayout {
    pub(super) fn close_dialog(&mut self) {
        self.dialog = None;
    }
}

pub(super) fn loading_spinner(id: impl Into<ElementId>, size: Pixels, color: Rgba) -> AnyElement {
    svg()
        .path("spinner")
        .size(size)
        .text_color(color)
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(750)).repeat(),
            |el, delta| el.with_transformation(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
}

fn btn_body(
    spinner_id: &'static str,
    label: impl Into<SharedString>,
    loading: bool,
    spinner_color: Rgba,
) -> Div {
    div()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .when(loading, |d| d.opacity(0.))
                .child(label.into()),
        )
        .when(loading, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(loading_spinner(spinner_id, px(15.), spinner_color)),
            )
        })
}

pub(super) fn route_btn(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    p: Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .h(crate::theme::text_px(32.)).flex().items_center().justify_center()
        .rounded_full()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .when(disabled, |d| {
            d.bg(p.fill()).text_color(p.muted()).opacity(0.6)
        })
        .when(!disabled, |d| {
            d.cursor_pointer()
                .bg(p.primary_fill())
                .text_color(rgb(0xffffff))
        })
        .child(label.into())
}

pub(super) fn action_btn(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    p: Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .h(crate::theme::text_px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(p.fill())
        .border_1()
        .border_color(p.control_rim())
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .when(disabled, |d| d.text_color(p.muted()).opacity(0.6))
        .when(!disabled, |d| {
            d.cursor_pointer()
                .hover(|s| s.bg(p.fill_strong()))
                .text_color(p.accent())
        })
        .child(label.into())
}

pub(super) fn danger_btn(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ghost: bool,
    disabled: bool,
    p: Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .h(crate::theme::text_px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(!ghost, |d| {
            d.bg(p.danger_soft())
                .border_1()
                .border_color(p.control_rim())
        })
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .when(disabled, |d| d.text_color(p.muted()).opacity(0.55))
        .when(!disabled, |d| {
            d.cursor_pointer()
                .hover(|s| {
                    s.bg(if ghost {
                        p.hover()
                    } else if p.0 {
                        rgba(0xff453a47)
                    } else {
                        rgba(0xff3b3033)
                    })
                })
                .text_color(p.danger())
        })
        .child(label.into())
}

pub(super) fn icon_btn(
    id: impl Into<ElementId>,
    glyph: &'static str,
    selected: bool,
    danger: bool,
    p: Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(28.))
        .rounded_lg()
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .when(selected, |d| d.bg(p.primary_soft()).text_color(p.accent()))
        .when(!selected, |d| {
            d.hover(|s| s.bg(p.fill_strong())).text_color(if danger {
                p.danger()
            } else {
                p.muted()
            })
        })
        .child(svg().path(glyph).size(px(15.)).text_color(if selected { p.accent() } else if danger { p.danger() } else { p.muted() }))
}

pub(super) fn divider(p: Palette) -> Div {
    div().flex_none().h(px(1.)).ml_4().bg(p.line())
}

pub(super) use crate::glass::glass_input;

pub(super) fn picker(
    id: &'static str,
    selected: String,
    options: Vec<(String, String)>,
    change: impl Fn(&mut Workspace, String, &mut Context<Workspace>) + 'static,
    cx: &Context<Workspace>,
) -> AnyElement {
    let label = options
        .iter()
        .find(|(v, _)| *v == selected)
        .map(|(_, s)| s.clone())
        .unwrap_or_else(|| "请选择".into());
    let view = cx.weak_entity();
    let change = std::rc::Rc::new(change);
    let palette = Palette::get(cx);
    let mut dropdown = DropdownBox::new(id, label, palette);
    for (val, label) in options {
        let checked = val == selected;
        let change = change.clone();
        let view = view.clone();
        dropdown = dropdown.item(label, checked, move |_, cx| {
            let _ = view.update(cx, |v, cx| change(v, val.clone(), cx));
        });
    }
    dropdown.into_any_element()
}

mod forms;

impl Workspace {
    fn expire_settings_undo(&self, started: std::time::Instant, cx: &mut Context<Self>) {
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(8))
                .await;
            let _ = weak.update(cx, |v, cx| {
                if v.settings_layout
                    .wallpaper_undo
                    .as_ref()
                    .is_some_and(|entry| entry.0 == started)
                {
                    v.settings_layout.wallpaper_undo = None;
                }
                if v.settings_layout
                    .mapping_undo
                    .as_ref()
                    .is_some_and(|entry| entry.0 == started)
                {
                    v.settings_layout.mapping_undo = None;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_wallpaper(&mut self, cx: &mut Context<Self>) {
        let result = (|| {
            let dir = crate::service::data_dir()?;
            let saved = singboard_core::settings::wallpaper_path(&dir);
            let backup = tempfile::tempdir_in(&dir).map_err(|e| e.to_string())?;
            std::fs::rename(saved, backup.path().join("wallpaper")).map_err(|e| e.to_string())?;
            Ok::<_, String>(backup)
        })();
        match result {
            Ok(backup) => {
                let started = std::time::Instant::now();
                self.settings_layout.wallpaper_undo = Some((started, backup));
                self.expire_settings_undo(started, cx);
                crate::wallpaper_editor::notify_changed();
            }
            Err(error) => self.message(error,true,cx),
        }
        cx.notify();
    }

    fn undo_wallpaper(&mut self, cx: &mut Context<Self>) {
        let Some((_, backup)) = &self.settings_layout.wallpaper_undo else {
            return;
        };
        let result = (|| {
            let dir = crate::service::data_dir()?;
            let mut source =
                std::fs::File::open(backup.path().join("wallpaper")).map_err(|e| e.to_string())?;
            let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
            std::io::copy(&mut source, &mut file).map_err(|e| e.to_string())?;
            file.persist_noclobber(singboard_core::settings::wallpaper_path(&dir))
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(())
        })();
        match result {
            Ok(()) => {
                self.settings_layout.wallpaper_undo = None;
                crate::wallpaper_editor::notify_changed();
            }
            Err(error) => self.message(error,true,cx),
        }
        cx.notify();
    }

    fn download_panel(&mut self, cx: &mut Context<Self>) {
        use std::sync::atomic::{AtomicU64, Ordering};
        let Some(panel) = self.settings_layout.panel_update.as_mut() else {
            return;
        };
        if panel.download.is_some() || panel.package.is_some() {
            return;
        }
        panel.error.clear();
        let version = panel.info.version.clone();
        let mirror = panel.info.mirror.clone();
        let received = Arc::new(AtomicU64::new(0));
        let progress = received.clone();
        let job = singboard_core::runtime().spawn(async move {
            singboard_core::update::download_with_progress(
                &crate::service::data_dir()?.join("updates"),
                &version,
                &crate::maintenance::preferences()?.github_token,
                &mirror,
                |bytes| progress.store(bytes, Ordering::Relaxed),
            )
            .await
        });
        panel.download = Some((received.clone(), job.abort_handle()));
        cx.spawn(async move |weak, cx| {
            while !job.is_finished() {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(120))
                    .await;
                if weak.update(cx, |_, cx| cx.notify()).is_err() {
                    job.abort();
                    return;
                }
            }
            let result = job.await.map_err(|e| e.to_string()).and_then(|v| v);
            let _ = weak.update(cx, |view, cx| {
                if let Some(panel) = &mut view.settings_layout.panel_update
                    && panel
                        .download
                        .as_ref()
                        .is_some_and(|(active, _)| Arc::ptr_eq(active, &received))
                {
                    panel.download = None;
                    match result {
                        Ok(package) => panel.package = Some(package),
                        Err(error) => panel.error = error,
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn dismiss_panel_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.settings_layout.panel_update.take() {
            if let Some((_, task)) = &panel.download {
                task.abort();
            }
            if let Some(focus) = &panel.previous_focus {
                window.focus(focus, cx);
            }
            self.close_update_dialog(panel, cx);
        }
    }

    fn confirm_panel_update(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = &mut self.settings_layout.panel_update else {
            return;
        };
        if let Some((path, digest)) = &panel.package {
            if crate::editor::has_pending_changes(cx) {
                panel.error = "请先保存配置，再重启更新。".into();
            } else if let Err(error) = crate::maintenance::launch_installer(path, digest) {
                panel.error = error;
            } else {
                cx.quit();
            }
            cx.notify();
        } else {
            panel.key_action = 1;
            self.download_panel(cx);
        }
    }

    pub(crate) fn panel_update_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (panel, leaving) = match (&mut self.settings_layout.panel_update, &mut self.leaving_dialog) {
            (Some(panel), _) => (panel, false),
            (None, Some((_, dialog::LeavingDialog::Update(panel)))) => (panel, true),
            _ => return None,
        };
        if !leaving && !panel.shown {
            panel.previous_focus = window.focused(cx);
            panel.shown = true;
            window.focus(&panel.focus, cx);
        }
        let p = Palette::get(cx);
        let downloading = panel.download.is_some();
        let ready = panel.package.is_some();
        let mut card = popover_card(p, self.glass_tint)
            .w_full()
            .max_w(px(420.))
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if downloading {
                        "正在更新面板"
                    } else if ready {
                        "新版面板已准备就绪"
                    } else {
                        if panel.info.reinstall {
                            "本地面板和官方发布的不同"
                        } else {
                            "发现新面板版本"
                        }
                    }),
            );
        for (label, value) in [
            ("当前", singboard_core::update::CURRENT),
            ("最新", panel.info.version.as_str()),
            ("发布时间", panel.info.published_at.as_str()),
            ("下载经由", panel.info.mirror.as_str()),
        ] {
            if !value.is_empty() {
                card = card.child(
                    div()
                        .flex()
                        .justify_between()
                        .gap_3()
                        .text_xs()
                        .child(div().text_color(p.muted()).child(label))
                        .child(div().font_family("Consolas").child(value.to_owned())),
                );
            }
        }
        if let Some((received, _)) = &panel.download {
            let bytes = received.load(std::sync::atomic::Ordering::Relaxed);
            let total = panel.info.size.max(1);
            card = card
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted())
                        .child(if bytes >= total {
                            "正在校验…".into()
                        } else {
                            format!(
                                "下载中… {:.1} MB / {:.1} MB",
                                bytes as f64 / 1048576.,
                                total as f64 / 1048576.
                            )
                        }),
                )
                .child(
                    div().w_full().h(px(5.)).rounded_full().bg(p.fill()).child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(p.primary_fill())
                            .w(relative((bytes as f32 / total as f32).min(1.))),
                    ),
                );
        }
        card = card
            .child(div().text_xs().text_color(p.muted()).child(if ready {
                "更新包已通过 SHA-256 校验。重启面板即可完成安装，代理服务不受影响。"
            } else if panel.info.reinstall {
                "版本相同，但本地文件和官方发布的文件不同。重新安装会关闭面板并自动重新打开，代理服务不受影响。"
            } else {
                "更新会关闭面板并自动重新打开，代理服务不受影响。"
            }))
            .when(!panel.error.is_empty(), |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(p.danger())
                        .child(panel.error.clone()),
                )
            });
        let actions = div()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .child(
                action_btn("panel-changelog", "查看更新日志", false, p)
                    .when(panel.keyboard && panel.key_action == 0, |d| {
                        d.border_1().border_color(p.accent())
                    })
                    .on_click(|_, _, cx| {
                        cx.open_url("https://github.com/Yuu518/singboard/releases");
                    }),
            )
            .child(
                action_btn(
                    "panel-later",
                    if downloading {
                        "取消下载"
                    } else {
                        "稍后"
                    },
                    false,
                    p,
                )
                .when(panel.keyboard && panel.key_action == 1, |d| {
                    d.border_1().border_color(p.accent())
                })
                .on_click(cx.listener(|v, _, w, cx| v.dismiss_panel_update(w, cx))),
            )
            .when(!downloading, |d| {
                d.child(
                    route_btn(
                        "panel-confirm",
                        if ready {
                            "重启更新"
                        } else {
                            if panel.info.reinstall {
                                "重新安装"
                            } else {
                                "立即更新"
                            }
                        },
                        false,
                        p,
                    )
                    .when(panel.keyboard && panel.key_action == 2, |d| {
                        d.border_1().border_color(p.ink())
                    })
                    .on_click(cx.listener(|v, _, _, cx| v.confirm_panel_update(cx))),
                )
            });
        Some(dialog::surface(
            "panel-pop",
            div()
                .id("panel-update-overlay")
                .occlude()
                .track_focus(&panel.focus)
                .capture_key_down(cx.listener(|v, event: &KeyDownEvent, w, cx| {
                    let Some(panel) = v.settings_layout.panel_update.as_mut() else {
                        return;
                    };
                    match event.keystroke.key.as_str() {
                        "escape" => v.dismiss_panel_update(w, cx),
                        "tab" => {
                            let count = if panel.download.is_some() { 2 } else { 3 };
                            panel.keyboard = true;
                            panel.key_action = if event.keystroke.modifiers.shift {
                                (panel.key_action + count - 1) % count
                            } else {
                                (panel.key_action + 1) % count
                            };
                            cx.notify();
                        }
                        "enter" | "space" => match panel.key_action {
                            0 => cx.open_url("https://github.com/Yuu518/singboard/releases"),
                            1 => v.dismiss_panel_update(w, cx),
                            _ => v.confirm_panel_update(cx),
                        },
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                .absolute()
                .inset_0()
                .size_full()
                .bg(dialog::scrim_color(p.0))
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                ,
            card.child(actions), 420., leaving, cx,
        ))
    }
}

const START_PAGES: [(&str, &str); 7] = [
    ("/overview", "概览"),
    ("/proxies", "代理"),
    ("/connections", "连接"),
    ("/logs", "日志"),
    ("/rules", "规则"),
    ("/config", "配置"),
    ("/settings", "设置"),
];

const FONT_SCALE_MIN: f32 = 0.9;
const FONT_SCALE_MAX: f32 = 1.4;

fn font_scale_pos(scale: f32) -> f32 {
    ((scale - FONT_SCALE_MIN) / (FONT_SCALE_MAX - FONT_SCALE_MIN)).clamp(0., 1.)
}

fn snap_font_scale(pos: f32) -> f32 {
    let clamped = pos.clamp(0., 1.);
    if (clamped - 0.2).abs() <= 0.012 {
        return 1.0;
    }
    let smooth = (clamped * 200.).round() / 200.;
    FONT_SCALE_MIN + (FONT_SCALE_MAX - FONT_SCALE_MIN) * smooth
}

fn font_scale_label(scale: f32) -> String {
    let pct = (scale * 100.).round() as u32;
    if pct == 100 {
        "标准 · 100%".into()
    } else {
        format!("{pct}%")
    }
}

fn snap_tint(value: f32) -> f32 {
    let clamped = value.clamp(0., 1.);
    if (clamped - 0.5).abs() <= 0.012 {
        return 0.5;
    }
    (clamped * 200.).round() / 200.
}

fn glass_tint_label(tint: f32) -> &'static str {
    match tint {
        0. => "通透",
        0.5 => "适中",
        1. => "模糊",
        value if value < 0.5 => "偏通透",
        _ => "偏模糊",
    }
}

fn mode_option_label(mode: &str) -> String {
    match mode.to_lowercase().as_str() {
        "rule" => "规则".into(),
        "global" => "全局".into(),
        "direct" => "直连".into(),
        _ => mode.to_owned(),
    }
}

fn section_header(title: &'static str, palette: crate::theme::Palette, cx: &App) -> Div {
    crate::glass::backdrop_pill(title, true, palette, cx)
        .px_1()
        .pb_2()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
}

pub(super) fn setting_row(
    title: &'static str,
    desc: impl Into<SharedString>,
    right: AnyElement,
    stacked: bool,
    palette: crate::theme::Palette,
) -> Div {
    let desc_str: SharedString = desc.into();
    div()
        .w_full()
        .min_h(px(56.))
        .px_4()
        .py_3()
        .flex()
        .when(stacked, |r| r.flex_col().items_stretch().gap(px(10.)))
        .when(!stacked, |r| r.items_center().justify_between().gap_4())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(crate::theme::text_px(13.)).line_height(rems(18. / 16.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
                .when(!desc_str.is_empty(), |el| {
                    el.child(
                        div()
                            .text_xs()
                            .line_height(crate::theme::text_px(17.))
                            .text_color(palette.muted())
                            .child(desc_str),
                    )
                }),
        )
        .child(right)
}

static AUTO_LAUNCH_CACHE: std::sync::Mutex<Option<bool>> = std::sync::Mutex::new(None);

fn cached_auto_launch() -> bool {
    if let Ok(guard) = AUTO_LAUNCH_CACHE.lock()
        && let Some(val) = *guard
    {
        return val;
    }
    let val = singboard_core::settings::get_auto_launch();
    if let Ok(mut guard) = AUTO_LAUNCH_CACHE.lock() {
        *guard = Some(val);
    }
    val
}

fn set_cached_auto_launch(enabled: bool) {
    if let Ok(mut guard) = AUTO_LAUNCH_CACHE.lock() {
        *guard = Some(enabled);
    }
}

impl Workspace {
    pub(super) fn settings_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let palette = crate::theme::Palette::get(cx);
        let translucent = true;
        let tint = self.glass_tint;
        let prefs = crate::maintenance::preferences().unwrap_or_default();
        let current_theme = cx.global::<crate::theme::Appearance>().mode;
        let current_font_scale = self
            .settings_layout
            .font_draft
            .unwrap_or_else(|| cx.global::<crate::theme::Appearance>().font_scale)
            .clamp(FONT_SCALE_MIN, FONT_SCALE_MAX);
        let auto_launch = cached_auto_launch();
        let close_to_tray = prefs.close_to_tray.unwrap_or(false);
        let vp = window.viewport_size();
        let width = (f32::from(vp.width) - 256.).min(760.);
        let narrow = width <= 560.;
        let row = |title, desc: String, control| setting_row(title, desc, control, narrow, palette);
        let card = || {
            glass_card(palette, translucent, tint)
                .w_full()
                .flex()
                .flex_col()
                .flex_none()
        };
        let win_ratio = (f32::from(vp.width) / f32::from(vp.height).max(1.)).clamp(0.5, 3.0);

        let local_running = self.local_state == CoreState::Running;
        let local_stopped = self.local_state.is_stopped();
        let local_not_installed = self.local_state == CoreState::NotInstalled;
        let connected = self.api_connected();
        let svc_action = self.settings_layout.service_action;
        let svc_busy = svc_action.is_some() || self.settings_layout.pending.contains("service-op");
        let transition = self.service_transition();
        let (dot_color, halo_color) = if transition.is_some() {
            (palette.warning_dot(), palette.warning_soft())
        } else if local_running {
            (palette.success_dot(), palette.success_soft())
        } else if local_stopped {
            (palette.danger_fill(), palette.danger_soft())
        } else {
            (palette.muted(), rgba(0x00000000).into())
        };

        let start_loading = svc_action == Some("start");
        let restart_loading = svc_action == Some("restart");
        let stop_loading = svc_action == Some("stop");
        let start_disabled = svc_busy || local_running || local_not_installed;
        let restart_disabled = svc_busy || local_not_installed;
        let stop_disabled = svc_busy || local_stopped || local_not_installed;
        let svc_controls = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .id("set-svc-start")
                    .px_3()
                    .h(crate::theme::text_px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .text_size(crate::theme::text_px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .when(start_disabled, |d| {
                        d.bg(palette.fill()).text_color(palette.muted()).opacity(0.6)
                    })
                    .when(!start_disabled, |d| {
                        d.cursor_pointer()
                            .bg(palette.primary_fill())
                            .text_color(rgb(0xffffff))
                    })
                    .child(btn_body(
                        "spin-svc-start",
                        "启动",
                        start_loading,
                        palette.muted(),
                    ))
                    .when(!start_disabled, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.tray_service("start", cx)))
                    }),
            )
            .child(
                div()
                    .id("set-svc-restart")
                    .px_3()
                    .h(crate::theme::text_px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(palette.fill())
                    .border_1()
                    .border_color(palette.control_rim())
                    .text_size(crate::theme::text_px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .when(restart_disabled, |d| d.text_color(palette.muted()).opacity(0.6))
                    .when(!restart_disabled, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.bg(palette.fill_strong()))
                            .text_color(palette.accent())
                    })
                    .child(btn_body(
                        "spin-svc-restart",
                        "重启",
                        restart_loading,
                        palette.muted(),
                    ))
                    .when(!restart_disabled, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.tray_service("restart", cx)))
                    }),
            )
            .child(
                div()
                    .id("set-svc-stop")
                    .px_3()
                    .h(crate::theme::text_px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(palette.danger_soft())
                    .border_1()
                    .border_color(palette.control_rim())
                    .text_size(crate::theme::text_px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .when(stop_disabled, |d| d.text_color(palette.muted()).opacity(0.55))
                    .when(!stop_disabled, |d| {
                        d.cursor_pointer()
                            .hover(|s| {
                                s.bg(if palette.0 {
                                    rgba(0xff453a47)
                                } else {
                                    rgba(0xff3b3033)
                                })
                            })
                            .text_color(palette.danger())
                    })
                    .child(btn_body(
                        "spin-svc-stop",
                        "停止",
                        stop_loading,
                        palette.muted(),
                    ))
                    .when(!stop_disabled, |b| {
                        b.on_click(cx.listener(|this, _, _, cx| {
                            this.settings_layout.dialog = Some(SettingsDialog::ConfirmStop);
                            cx.notify();
                        }))
                    }),
            );

        let mode_desc = if !connected {
            "连接 Clash API 后可切换代理模式。".to_owned()
        } else if self.modes.is_empty() {
            "正在获取核心代理模式…".to_owned()
        } else {
            "核心当前采用的流量处理策略。".to_owned()
        };
        let mode_control: AnyElement = if !connected || self.modes.is_empty() {
            div()
                .when(!narrow, |d| d.w(px(240.)))
                .px_3()
                .py(px(6.))
                .rounded_lg()
                .bg(palette.fill())
                .text_xs()
                .text_color(palette.muted())
                .child(if !connected {
                    self.api_status_label().into()
                } else if self.mode.is_empty() {
                    "加载中…".into()
                } else {
                    mode_option_label(&self.mode)
                })
                .into_any_element()
        } else {
            div()
                .when(!narrow, |d| d.w(px(240.)))
                .child(picker(
                    "set-mode-picker",
                    self.mode.clone(),
                    self.modes
                        .iter()
                        .map(|m| (m.clone(), mode_option_label(m)))
                        .collect(),
                    |this, mode, cx| {
                        this.run(Command::SetMode { mode }, cx);
                    },
                    cx,
                ))
                .into_any_element()
        };

        let install_loading = svc_action == Some("install");
        let uninstall_loading = svc_action == Some("uninstall");
        let install_disabled = svc_busy || !local_not_installed;
        let uninstall_disabled = svc_busy || local_not_installed;
        let core_card = card()
            .child(
                div()
                    .w_full()
                    .min_h(px(56.))
                    .px_4()
                    .py_3()
                    .flex()
                    .when(narrow, |r| r.flex_col().items_stretch().gap(px(10.)))
                    .when(!narrow, |r| r.items_center().justify_between().gap_4())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(14.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(halo_color)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(div().size(px(8.)).rounded_full().bg(dot_color)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(crate::theme::text_px(14.)).line_height(crate::theme::text_px(20.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(self.core_status_label()),
                                    )
                                    .when_some(self.uptime().filter(|_| local_running), |el, uptime| {
                                        el.child(
                                            div()
                                                .text_xs().line_height(crate::theme::text_px(17.))
                                                .text_color(palette.muted())
                                                .child(format!("运行时间 {uptime}")),
                                        )
                                    }),
                            ),
                    )
                    .child(svc_controls),
            )
            .when_some(self.core_error.as_ref().filter(|_| transition.is_none()), |el, error| {
                el.child(div().px_4().pb_2().text_xs().text_color(palette.danger()).child(error.clone()))
            })
            .child(divider(palette))
            .child(row("代理模式", mode_desc, mode_control))
            .child(divider(palette))
            .child(self.disclosure(
                "service",
                "服务参数",
                "核心路径、工作目录与启动延迟。路径和目录在安装服务时写入，修改后需重新安装服务才生效。"
                    .into(),
                cx,
            ))
            .when(self.settings_layout.open.contains("service"), |card| {
                card.child(self.service_parameters(&prefs, narrow, window, cx))
            })
            .child(divider(palette))
            .child(row(
                "Windows 服务",
                "安装后，核心可独立于面板在后台运行。".into(),
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("install-service")
                            .px_3()
                            .h(crate::theme::text_px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(palette.fill())
                            .border_1()
                            .border_color(palette.control_rim())
                            .text_size(crate::theme::text_px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .when(install_disabled, |d| d.text_color(palette.muted()).opacity(0.6))
                            .when(!install_disabled, |d| {
                                d.cursor_pointer()
                                    .hover(|s| s.bg(palette.fill_strong()))
                                    .text_color(palette.accent())
                            })
                            .child(btn_body(
                                "spin-svc-install",
                                "安装服务",
                                install_loading,
                                palette.muted(),
                            ))
                            .when(!install_disabled, |b| {
                                b.on_click(cx.listener(|this, _, _, cx| {
                                    this.install_windows_service(cx);
                                }))
                            }),
                    )
                    .child(
                        div()
                            .id("uninstall-service")
                            .px_3()
                            .h(crate::theme::text_px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .text_size(crate::theme::text_px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .when(uninstall_disabled, |d| d.text_color(palette.muted()).opacity(0.55))
                            .when(!uninstall_disabled, |d| {
                                d.cursor_pointer()
                                    .hover(|s| s.bg(palette.hover()))
                                    .text_color(palette.danger())
                            })
                            .child(btn_body(
                                "spin-svc-uninstall",
                                "卸载服务",
                                uninstall_loading,
                                palette.muted(),
                            ))
                            .when(!uninstall_disabled, |b| {
                                b.on_click(cx.listener(move |v, _, _, cx| {
                                    v.settings_layout.dialog =
                                        Some(SettingsDialog::ConfirmUninstall {
                                            running: local_running,
                                        });
                                    cx.notify();
                                }))
                            }),
                    )
                    .into_any_element(),
            ))
            .child(self.result("service-op", palette));

        let api_card = card().child(self.endpoint_settings(&prefs, narrow, window, cx));
        let network_card = card().child(self.network_settings(&prefs, narrow, window, cx));
        let dns_card = card().child(self.dns_settings(&prefs, width, window, cx));

        let mut theme_seg = div()
            .flex()
            .gap(px(2.))
            .p(px(2.))
            .rounded_full()
            .bg(palette.fill())
            .border_1()
            .border_color(palette.control_rim());
        for (i, (mode, name)) in [
            crate::theme::MODES[2],
            crate::theme::MODES[0],
            crate::theme::MODES[1],
        ]
        .into_iter()
        .enumerate()
        {
            theme_seg = theme_seg.child(
                div()
                    .id(("set-theme", i))
                    .px(px(14.))
                    .py(px(3.))
                    .text_xs()
                    .rounded_full()
                    .cursor_pointer()
                    .when(current_theme == mode, |v| {
                        v.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(current_theme != mode, |v| {
                        v.text_color(palette.ink().opacity(0.75))
                    })
                    .child(name)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Err(error) = crate::theme::Appearance::select(
                            mode,
                            window,
                            this.custom_wallpaper,
                            cx,
                        ) {
                            this.message(error,true,cx);
                        }
                        cx.notify();
                    })),
            );
        }

        let font_focus = self
            .settings_layout
            .font_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let font_bounds = self.settings_layout.font_bounds.clone();
        let font_dragging = self.settings_layout.font_drag.is_some();
        let font_anchor_y = self.settings_layout.font_anchor_y;
        let font_scroll = self.page_scroll.clone();
        let font_pos = font_scale_pos(current_font_scale);
        let font_move_ws = cx.entity().downgrade();
        let font_up_ws = cx.entity().downgrade();
        let font_anchor_ws = cx.entity().downgrade();
        if font_dragging {
            self.settings_layout.font_preview_last = current_font_scale;
        }
        let preview_target = if font_dragging { 1. } else { 0. };
        let preview_dt = self
            .settings_layout
            .font_preview_tick
            .map(|t| t.elapsed().as_secs_f32().min(0.05))
            .unwrap_or(0.016);
        self.settings_layout.font_preview_tick = Some(std::time::Instant::now());
        if cx.reduce_motion() {
            self.settings_layout.font_preview_alpha = preview_target;
        } else if (preview_target - self.settings_layout.font_preview_alpha).abs() > 0.002 {
            let step = preview_dt / if font_dragging { 0.14 } else { 0.16 };
            self.settings_layout.font_preview_alpha = if font_dragging {
                (self.settings_layout.font_preview_alpha + step).min(1.)
            } else {
                (self.settings_layout.font_preview_alpha - step).max(0.)
            };
            if (preview_target - self.settings_layout.font_preview_alpha).abs() > 0.002 {
                window.request_animation_frame();
            } else {
                self.settings_layout.font_preview_alpha = preview_target;
            }
        } else {
            self.settings_layout.font_preview_alpha = preview_target;
        }
        let font_preview_alpha = self.settings_layout.font_preview_alpha;

        let font_scale_row = div()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(13.))
                                    .line_height(crate::theme::text_px(18.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("字体大小"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(palette.muted())
                                    .child(font_scale_label(current_font_scale)),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .line_height(crate::theme::text_px(17.))
                            .text_color(palette.muted())
                            .child("拖动滑条预览字号，松开后应用到整个界面，与系统文本缩放叠加。"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .w(px(22.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.muted())
                            .child("A"),
                    )
                    .child(
                        div()
                            .id("font-scale-slider")
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h(px(36.))
                            .cursor_pointer()
                            .track_focus(&font_focus)
                            .role(Role::Slider)
                            .aria_label(format!(
                                "字体大小 {}",
                                font_scale_label(current_font_scale)
                            ))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |v, event: &MouseDownEvent, window, cx| {
                                    let area = v.settings_layout.font_bounds.get();
                                    let left = area.origin.x + px(20.);
                                    let range = (area.size.width - px(40.)).max(px(1.));
                                    let cur = v
                                        .settings_layout
                                        .font_draft
                                        .unwrap_or_else(|| {
                                            cx.global::<crate::theme::Appearance>().font_scale
                                        });
                                    let thumb_x = left + range * font_scale_pos(cur);
                                    if (event.position.x - thumb_x).abs() <= px(20.) {
                                        v.settings_layout.font_drag =
                                            Some(event.position.x - thumb_x);
                                        v.settings_layout.font_draft = Some(cur);
                                    } else {
                                        v.settings_layout.font_drag = Some(px(0.));
                                        let next = snap_font_scale(
                                            f32::from(event.position.x - left) / f32::from(range),
                                        );
                                        v.settings_layout.font_draft = Some(next);
                                    }
                                    if let Some(focus) = v.settings_layout.font_focus.clone() {
                                        window.focus(&focus, cx);
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .capture_key_down(cx.listener(|v, event: &KeyDownEvent, window, cx| {
                                let cur = v.settings_layout.font_draft.unwrap_or_else(|| {
                                    cx.global::<crate::theme::Appearance>().font_scale
                                });
                                let next = match event.keystroke.key.as_str() {
                                    "left" | "down" => cur - 0.02,
                                    "right" | "up" => cur + 0.02,
                                    "pageup" => cur + 0.10,
                                    "pagedown" => cur - 0.10,
                                    "home" => FONT_SCALE_MIN,
                                    "end" => FONT_SCALE_MAX,
                                    _ => return,
                                };
                                let value = snap_font_scale(font_scale_pos(next));
                                let area = v.settings_layout.font_bounds.get();
                                if area.size.height > px(0.) {
                                    v.settings_layout.font_anchor_y = Some(area.origin.y);
                                }
                                v.settings_layout.font_draft = None;
                                if let Err(error) = crate::theme::Appearance::select_font_scale(
                                    value,
                                    window,
                                    v.custom_wallpaper,
                                    cx,
                                ) {
                                    v.message(error, true, cx);
                                }
                                window.prevent_default();
                                cx.stop_propagation();
                                cx.notify();
                            }))
                            .child(
                                canvas(
                                    |_, _, _| (),
                                    move |area, _, window, cx| {
                                        font_bounds.set(area);
                                        if let Some(target_y) = font_anchor_y {
                                            let delta = area.origin.y - target_y;
                                            if delta.abs() > px(0.5) {
                                                let off = font_scroll.offset();
                                                font_scroll.set_offset(point(
                                                    off.x,
                                                    (off.y - delta).min(px(0.)),
                                                ));
                                                window.request_animation_frame();
                                            }
                                            let font_anchor_ws = font_anchor_ws.clone();
                                            cx.defer(move |cx| {
                                                let _ = font_anchor_ws.update(cx, |v, _| {
                                                    v.settings_layout.font_anchor_y = None;
                                                });
                                            });
                                        }
                                        let range = (area.size.width - px(40.)).max(px(0.));
                                        let left = area.origin.x + px(20.);
                                        let center = area.origin.y + area.size.height * 0.5;
                                        let x = left + range * font_pos;
                                        let track = |start, end, color| {
                                            quad(
                                                Bounds::new(
                                                    point(start, center - px(2.)),
                                                    size(end - start, px(4.)),
                                                ),
                                                px(2.),
                                                color,
                                                px(0.),
                                                color,
                                                BorderStyle::default(),
                                            )
                                        };
                                        window.paint_quad(track(
                                            area.origin.x,
                                            area.origin.x + area.size.width,
                                            palette.fill_strong(),
                                        ));
                                        window.paint_quad(track(
                                            area.origin.x,
                                            x,
                                            palette.primary_fill(),
                                        ));
                                        for tick in [0., 0.2, 0.6, 1.] {
                                            if (font_pos - tick).abs() >= 0.055 {
                                                window.paint_quad(quad(
                                                    Bounds::new(
                                                        point(
                                                            left + range * tick - px(1.75),
                                                            center + px(9.),
                                                        ),
                                                        size(px(3.5), px(3.5)),
                                                    ),
                                                    px(1.75),
                                                    palette.muted().opacity(0.42),
                                                    px(0.),
                                                    transparent_black(),
                                                    BorderStyle::default(),
                                                ));
                                            }
                                        }
                                        let (tw, th, tr) = if font_dragging {
                                            (px(44.), px(28.), px(14.))
                                        } else {
                                            (px(40.), px(26.), px(13.))
                                        };
                                        let thumb = Bounds::new(
                                            point(x - tw * 0.5, center - th * 0.5),
                                            size(tw, th),
                                        );
                                        window.paint_drop_shadows(
                                            thumb,
                                            tr.into(),
                                            &[BoxShadow {
                                                color: hsla(
                                                    0.,
                                                    0.,
                                                    0.,
                                                    if palette.0 {
                                                        0.36
                                                    } else if font_dragging {
                                                        0.14
                                                    } else {
                                                        0.18
                                                    },
                                                ),
                                                offset: point(px(0.), px(2.)),
                                                blur_radius: px(if font_dragging { 8. } else { 6. }),
                                                spread_radius: px(-1.),
                                                inset: false,
                                            }],
                                        );
                                        if font_dragging {
                                            window.paint_quad(quad(
                                                thumb,
                                                tr,
                                                rgba(0xffffff38),
                                                px(1.2),
                                                rgba(0xffffffd8),
                                                BorderStyle::Solid,
                                            ));
                                            window.paint_quad(quad(
                                                thumb,
                                                tr,
                                                transparent_black(),
                                                px(0.6),
                                                rgba(0x00000022),
                                                BorderStyle::Solid,
                                            ));
                                        } else {
                                            window.paint_quad(quad(
                                                thumb,
                                                tr,
                                                rgb(0xffffff),
                                                px(0.5),
                                                rgba(0x00000018),
                                                BorderStyle::Solid,
                                            ));
                                        }
                                        if font_dragging {
                                            let font_move_ws = font_move_ws.clone();
                                            window.on_mouse_event(
                                                move |event: &MouseMoveEvent, phase, window, cx| {
                                                    if phase != DispatchPhase::Bubble {
                                                        return;
                                                    }
                                                    let _ = font_move_ws.update(cx, |v, cx| {
                                                        let Some(grab) =
                                                            v.settings_layout.font_drag
                                                        else {
                                                            return;
                                                        };
                                                        if event.pressed_button
                                                            != Some(MouseButton::Left)
                                                        {
                                                            v.settings_layout.font_drag = None;
                                                            if let Some(value) =
                                                                v.settings_layout.font_draft.take()
                                                            {
                                                                v.settings_layout.font_anchor_y =
                                                                    Some(area.origin.y);
                                                                if let Err(error) = crate::theme::Appearance::select_font_scale(
                                                                    value,
                                                                    window,
                                                                    v.custom_wallpaper,
                                                                    cx,
                                                                ) {
                                                                    v.message(error, true, cx);
                                                                }
                                                            }
                                                            cx.notify();
                                                            return;
                                                        }
                                                        let range =
                                                            (area.size.width - px(40.)).max(px(1.));
                                                        let left = area.origin.x + px(20.);
                                                        v.settings_layout.font_draft =
                                                            Some(snap_font_scale(
                                                                f32::from(
                                                                    event.position.x - grab - left,
                                                                ) / f32::from(range),
                                                            ));
                                                        cx.stop_propagation();
                                                        cx.notify();
                                                    });
                                                },
                                            );
                                            let font_up_ws = font_up_ws.clone();
                                            window.on_mouse_event(
                                                move |event: &MouseUpEvent, phase, window, cx| {
                                                    if phase != DispatchPhase::Bubble
                                                        || event.button != MouseButton::Left
                                                    {
                                                        return;
                                                    }
                                                    let _ = font_up_ws.update(cx, |v, cx| {
                                                        if v.settings_layout.font_drag.take().is_some()
                                                        {
                                                            if let Some(value) =
                                                                v.settings_layout.font_draft.take()
                                                            {
                                                                v.settings_layout.font_anchor_y =
                                                                    Some(area.origin.y);
                                                                if let Err(error) = crate::theme::Appearance::select_font_scale(
                                                                    value,
                                                                    window,
                                                                    v.custom_wallpaper,
                                                                    cx,
                                                                ) {
                                                                    v.message(error, true, cx);
                                                                }
                                                            }
                                                            cx.notify();
                                                        }
                                                    });
                                                },
                                            );
                                        }
                                    },
                                )
                                .size_full(),
                            ),
                    )
                    .child(
                        div()
                            .w(px(22.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(19.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.muted())
                            .child("A"),
                    ),
            );

        let start_idx = START_PAGES
            .iter()
            .position(|(p, _)| *p == prefs.start_page)
            .unwrap_or(0);
        let tint_focus = self
            .settings_layout
            .tint_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let tint_bounds = self.settings_layout.tint_bounds.clone();
        let tint_dragging = self.settings_layout.tint_drag.is_some();
        let move_ws = cx.entity().downgrade();
        let up_ws = cx.entity().downgrade();

        let personalization_subpanel = div()
            .mx_4()
            .mb_3()
            .rounded(px(14.))
            .bg(palette.fill())
            .border_1()
            .border_color(palette.line())
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("玻璃效果"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .line_height(crate::theme::text_px(17.))
                                    .text_color(palette.muted())
                                    .child(
                                        "作用于顶部栏、侧栏、卡片和弹窗。往左更通透，偏通透时带折射；往右底色更重、文字更易读。",
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center().gap(px(12.))
                            .child(svg().path("glass-clear").size(px(22.)).flex_none().text_color(palette.muted()))
                            .child(
                                div()
                                    .id("glass-tint-slider")
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(36.))
                                    .cursor_pointer()
                                    .track_focus(&tint_focus)
                                    .role(Role::Slider)
                                    .aria_label(format!(
                                        "玻璃效果 {}，{}%",
                                        glass_tint_label(tint),
                                        (100. * tint).round()
                                    ))
                                    .on_mouse_down(MouseButton::Left, cx.listener(move |v, event: &MouseDownEvent, window, cx| {
                                        let area = v.settings_layout.tint_bounds.get();
                                        let left = area.origin.x + px(20.);
                                        let range = (area.size.width - px(40.)).max(px(1.));
                                        let thumb_x = left + range * v.glass_tint;
                                        if (event.position.x - thumb_x).abs() <= px(20.) {
                                            v.settings_layout.tint_drag = Some(event.position.x - thumb_x);
                                        } else {
                                            v.settings_layout.tint_drag = Some(px(0.));
                                            v.glass_tint = snap_tint(f32::from(event.position.x - left) / f32::from(range));
                                        }
                                        if let Some(focus) = v.settings_layout.tint_focus.clone() {
                                            window.focus(&focus, cx);
                                        }
                                        cx.stop_propagation();
                                        cx.notify();
                                    }))
                                    .capture_key_down(cx.listener(|v, event: &KeyDownEvent, window, cx| {
                                        let value = match event.keystroke.key.as_str() {
                                            "left" | "down" => v.glass_tint - 0.05,
                                            "right" | "up" => v.glass_tint + 0.05,
                                            "pageup" => v.glass_tint + 0.25,
                                            "pagedown" => v.glass_tint - 0.25,
                                            "home" => 0.,
                                            "end" => 1.,
                                            _ => return,
                                        };
                                        let value = snap_tint(value);
                                        v.glass_tint = value;
                                        if let Err(error) = crate::service::data_dir().and_then(|dir| Settings::set_glass_tint(&dir, value)) {
                                            v.message(error, true, cx);
                                        }
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        cx.notify();
                                    }))
                                    .child(canvas(|_, _, _| (), move |area, _, window, _| {
                                        tint_bounds.set(area);
                                        let range = (area.size.width - px(40.)).max(px(0.));
                                        let left = area.origin.x + px(20.);
                                        let center = area.origin.y + area.size.height * 0.5;
                                        let x = left + range * tint;
                                        let track = |start, end, color| quad(Bounds::new(point(start, center - px(2.)), size(end - start, px(4.))), px(2.), color, px(0.), color, BorderStyle::default());
                                        window.paint_quad(track(area.origin.x, area.origin.x + area.size.width, palette.fill_strong()));
                                        window.paint_quad(track(area.origin.x, x, palette.primary_fill()));
                                        for tick in [0., 0.5, 1.] {
                                            if (tint - tick).abs() >= 0.055 {
                                                window.paint_quad(quad(Bounds::new(point(left + range * tick - px(1.75), center + px(9.)), size(px(3.5), px(3.5))), px(1.75), palette.muted().opacity(0.42), px(0.), transparent_black(), BorderStyle::default()));
                                            }
                                        }
                                        let (tw, th, tr) = if tint_dragging { (px(44.), px(28.), px(14.)) } else { (px(40.), px(26.), px(13.)) };
                                        let thumb = Bounds::new(point(x - tw * 0.5, center - th * 0.5), size(tw, th));
                                        window.paint_drop_shadows(
                                            thumb,
                                            tr.into(),
                                            &[BoxShadow {
                                                color: hsla(0., 0., 0., if palette.0 { 0.36 } else if tint_dragging { 0.14 } else { 0.18 }),
                                                offset: point(px(0.), px(2.)),
                                                blur_radius: px(if tint_dragging { 8. } else { 6. }),
                                                spread_radius: px(-1.),
                                                inset: false,
                                            }],
                                        );
                                        if tint_dragging {
                                            window.paint_quad(quad(thumb, tr, rgba(0xffffff38), px(1.2), rgba(0xffffffd8), BorderStyle::Solid));
                                            window.paint_quad(quad(thumb, tr, transparent_black(), px(0.6), rgba(0x00000022), BorderStyle::Solid));
                                        } else {
                                            window.paint_quad(quad(thumb, tr, rgb(0xffffff), px(0.5), rgba(0x00000018), BorderStyle::Solid));
                                        }
                                        if tint_dragging {
                                            let move_ws = move_ws.clone();
                                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                                if phase != DispatchPhase::Bubble {
                                                    return;
                                                }
                                                let _ = move_ws.update(cx, |v, cx| {
                                                    let Some(grab) = v.settings_layout.tint_drag else {
                                                        return;
                                                    };
                                                    if event.pressed_button != Some(MouseButton::Left) {
                                                        v.settings_layout.tint_drag = None;
                                                        let value = v.glass_tint;
                                                        if let Err(error) = crate::service::data_dir().and_then(|dir| Settings::set_glass_tint(&dir, value)) {
                                                            v.message(error, true, cx);
                                                        }
                                                        cx.notify();
                                                        return;
                                                    }
                                                    let range = (area.size.width - px(40.)).max(px(1.));
                                                    let left = area.origin.x + px(20.);
                                                    v.glass_tint = snap_tint(f32::from(event.position.x - grab - left) / f32::from(range));
                                                    cx.stop_propagation();
                                                    cx.notify();
                                                });
                                            });
                                            let up_ws = up_ws.clone();
                                            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                                                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                                                    return;
                                                }
                                                let _ = up_ws.update(cx, |v, cx| {
                                                    if v.settings_layout.tint_drag.take().is_some() {
                                                        let value = v.glass_tint;
                                                        if let Err(error) = crate::service::data_dir().and_then(|dir| Settings::set_glass_tint(&dir, value)) {
                                                            v.message(error, true, cx);
                                                        }
                                                        cx.notify();
                                                    }
                                                });
                                            });
                                        }
                                    }).size_full()),
                            )
                            .child(svg().path("glass-tinted").size(px(22.)).flex_none().text_color(palette.muted())),
                    ),
            )
            .child(div().h(px(0.5)).ml(px(14.)).bg(palette.line()))
            .child(
                div()
                    .px(px(14.))
                    .py(px(10.))
                    .min_h(px(52.))
                    .flex()
                    .when(narrow, |d| d.flex_col().items_stretch().gap(px(10.)))
                    .when(!narrow, |d| d.items_center().justify_between().gap_4())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("背景图片"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(palette.muted())
                                    .child(if self.custom_wallpaper {
                                        "显示在窗口背后，玻璃偏通透时会折射它。"
                                    } else {
                                        "未设置，当前使用默认流光背景。"
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap_2()
                            .when_some(
                                crate::glass::wallpaper_thumb(cx).filter(|_| self.custom_wallpaper),
                                |el, thumb| {
                                    el.child(
                                        div()
                                            .w(px(48.))
                                            .h(px(28.))
                                            .rounded(px(6.))
                                            .overflow_hidden()
                                            .border_1()
                                            .border_color(palette.rim())
                                            .child(img(thumb).size_full().object_fit(ObjectFit::Cover)),
                                    )
                                },
                            )
                            .child(
                                action_btn(
                                    "set-edit-wallpaper",
                                    if self.custom_wallpaper {
                                        "更换"
                                    } else {
                                        "选择图片"
                                    },
                                    false,
                                    palette,
                                )
                                .on_click(cx.listener(move |_, _, window, cx| {
                                    crate::wallpaper_editor::open(win_ratio, window,cx);
                                })),
                            )
                                .when(self.settings_layout.wallpaper_undo.is_some() && !self.custom_wallpaper, |el| {
                                    el.child(action_btn("undo-wallpaper", "撤销删除", false, palette)
                                        .on_click(cx.listener(|v, _, _, cx| v.undo_wallpaper(cx))))
                                })
                                .when(self.custom_wallpaper, |el| {
                                el.child(
                                    icon_btn("set-remove-wallpaper", "trash", false, true, palette)
                                        .on_click(cx.listener(|this, _, _, cx| this.remove_wallpaper(cx))),
                                )
                            }),
                    ),
            );

        let app_card = card()
            .child(row("界面主题", String::new(), theme_seg.into_any_element()))
            .child(divider(palette))
            .child(font_scale_row)
            .child(divider(palette))
            .child(row(
                "默认启动页",
                "启动面板时首先打开的页面，下次启动生效。".into(),
                div()
                    .when(!narrow, |d| d.w(px(240.)))
                    .child(picker(
                        "start-page",
                        START_PAGES[start_idx].0.into(),
                        START_PAGES
                            .iter()
                            .map(|(v, l)| (v.to_string(), l.to_string()))
                            .collect(),
                        |this, value, cx| {
                            this.edit_preferences(
                                |p| {
                                    p.start_page = value;
                                    Ok(())
                                },
                                cx,
                            )
                        },
                        cx,
                    ))
                    .into_any_element(),
            ))
            .child(divider(palette))
            .child(self.disclosure(
                "appearance",
                "个性化",
                format!(
                    "{} · {}",
                    glass_tint_label(tint),
                    if self.custom_wallpaper {
                        "已设置背景"
                    } else {
                        "默认背景"
                    }
                ),
                cx,
            ))
            .when(self.settings_layout.open.contains("appearance"), |el| {
                el.child(personalization_subpanel)
            })
            .child(divider(palette))
            .child(setting_row(
                "关闭到系统托盘",
                "关闭主窗口时让面板继续在后台运行。",
                pill_switch("set-toggle-tray", close_to_tray, palette)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Ok(dir) = crate::service::data_dir() {
                            if let Err(e) = singboard_core::settings::Settings::edit(&dir, |p| {
                                p.close_to_tray = Some(!close_to_tray);
                                Ok(())
                            }) {
                                this.message(e,true,cx);
                            }
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
                false,
                palette,
            ))
            .child(divider(palette))
            .child(setting_row(
                "开机启动面板",
                "登录 Windows 后自动启动 singboard。",
                pill_switch("set-toggle-autolaunch", auto_launch, palette)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match singboard_core::settings::set_auto_launch(!auto_launch) {
                            Ok(()) => {
                                set_cached_auto_launch(!auto_launch);
                                this.message(if !auto_launch {
                                    "已启用开机启动面板".into()
                                } else {
                                    "已关闭开机启动面板".into()
                                },false,cx);
                            }
                            Err(e) => this.message(e,true,cx),
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
                false,
                palette,
            ));

        let (core_update, panel_update) = self.update_settings(&prefs, narrow, window, cx);

        let content = div()
            .w_full()
            .max_w(px(760.))
            .mx_auto()
            .flex()
            .flex_col()
            .pb_8()
            .child(
                crate::glass::backdrop_heading("设置", palette, cx)
                    .pb(px(20.))
                    .text_size(crate::theme::text_px(26.))
                    .line_height(crate::theme::text_px(32.))
                    .font_weight(FontWeight::BOLD),
            )
            .child(section_header("核心与服务", palette, cx))
            .child(core_card)
            .child(section_header("API 设置", palette, cx).mt_6())
            .child(api_card)
            .child(section_header("网络测试", palette, cx).mt_6())
            .child(network_card)
            .child(section_header("DNS 查询", palette, cx).mt_6())
            .child(dns_card)
            .child(section_header("应用", palette, cx).mt_6())
            .child(app_card)
            .child(section_header("更新", palette, cx).mt_6())
            .child(card().child(core_update))
            .child(card().mt(px(10.)).child(panel_update));

        let font_preview = if font_preview_alpha > 0.002 {
            let eased = font_preview_alpha * font_preview_alpha * (3. - 2. * font_preview_alpha);
            let draft = self
                .settings_layout
                .font_draft
                .unwrap_or(self.settings_layout.font_preview_last)
                .clamp(FONT_SCALE_MIN, FONT_SCALE_MAX);
            let target_scale = (crate::theme::text_scale() * draft).clamp(0.9, 2.25);
            let area = self.settings_layout.font_bounds.get();
            let base_bottom = if area.size.height > px(0.) {
                (vp.height - area.origin.y + px(14.))
                    .clamp(px(64.), (vp.height - px(180.)).max(px(64.)))
            } else {
                px(96.)
            };
            let bottom_offset = base_bottom - px(6. * (1. - eased));
            Some(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(bottom_offset)
                    .px(px(20.))
                    .opacity(eased)
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(420.))
                            .rounded(px(16.))
                            .bg(palette.card(false))
                            .border_1()
                            .border_color(palette.rim())
                            .shadow_xl()
                            .p(px(14.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(palette.muted())
                                            .child("字号预览"),
                                    )
                                    .child(
                                        div()
                                            .px(px(8.))
                                            .py(px(2.))
                                            .rounded_full()
                                            .bg(palette.primary_soft())
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette.accent())
                                            .child(font_scale_label(draft)),
                                    ),
                            )
                            .child(
                                div()
                                    .rounded(px(12.))
                                    .bg(palette.fill())
                                    .p(px(12.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(
                                                        div()
                                                            .size(px(7.))
                                                            .rounded_full()
                                                            .bg(palette.success_dot()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(14. * target_scale))
                                                            .line_height(px(20. * target_scale))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(palette.ink())
                                                            .child("香港节点 01"),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12. * target_scale))
                                                    .line_height(px(18. * target_scale))
                                                    .text_color(palette.muted())
                                                    .child("42ms"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13. * target_scale))
                                            .line_height(px(19. * target_scale))
                                            .text_color(palette.ink())
                                            .child("概览 · 代理 · 连接 · 规则 · 设置"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12. * target_scale))
                                            .line_height(px(17. * target_scale))
                                            .text_color(palette.muted())
                                            .child("松开滑块后应用到整个界面"),
                                    ),
                            ),
                    ),
            )
        } else {
            None
        };

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(self.scroll_pane("settings-scroll", content, cx))
            .when_some(font_preview, |el, overlay| el.child(overlay))
    }
}
