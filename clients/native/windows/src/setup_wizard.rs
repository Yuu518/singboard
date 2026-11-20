use crate::{
    service,
    settings::Settings,
    theme::Palette,
    tools::{self, Values},
    workspace::{Page, Workspace},
};
use gpui::{prelude::*, *};
use gpui_component::input::InputState;

pub struct SetupWizard {
    pub visible: bool,
    initial_focus: bool,
    workspace: Entity<Workspace>,
    working_dir: Entity<InputState>,
    api_url: Entity<InputState>,
    api_secret: Entity<InputState>,
    api_id: String,
    api_name: String,
    error: String,
    saving: bool,
}

impl SetupWizard {
    pub fn new(
        prefs: &Settings,
        workspace: Entity<Workspace>,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let active = prefs.active();
        let default_url = active
            .map(|a| a.url.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:9090".into());
        let default_secret = active.map(|a| a.secret.clone()).unwrap_or_default();
        let api_id = active.map(|a| a.id.clone()).unwrap_or_default();
        let api_name = active
            .map(|a| a.name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "默认 API".into());
        let working_dir_val = prefs.working_dir.clone();

        let working_dir = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("C:\\sing-box")
                .default_value(working_dir_val)
        });
        let api_url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("http://127.0.0.1:9090")
                .default_value(default_url)
        });
        let api_secret = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("留空表示无密钥")
                .default_value(default_secret)
                .masked(true)
        });

        Self {
            visible,
            initial_focus: visible,
            workspace,
            working_dir,
            api_url,
            api_secret,
            api_id,
            api_name,
            error: String::new(),
            saving: false,
        }
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择 sing-box 工作目录".into()),
        });
        let handle = window.window_handle();
        cx.spawn(async move |weak, cx| {
            if let Ok(Ok(Some(paths))) = picker.await
                && let Some(dir) = paths.into_iter().next()
            {
                let value = dir.to_string_lossy().into_owned();
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        this.working_dir
                            .update(cx, |input, cx| input.set_value(value, window, cx));
                        this.error.clear();
                        cx.notify();
                    });
                });
            }
        })
        .detach();
    }

    fn go_to_settings(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.visible = false;
        self.workspace
            .update(cx, |ws, cx| ws.open(Page::Settings, cx));
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let working_dir = self.working_dir.read(cx).value().trim().to_owned();
        let api_url = self.api_url.read(cx).value().trim().to_owned();
        let api_secret = self.api_secret.read(cx).value().to_string();

        if working_dir.is_empty() {
            self.error = "请填写工作目录。".into();
            cx.notify();
            return;
        }
        if api_url.is_empty() {
            self.error = "请填写 Clash API 地址。".into();
            cx.notify();
            return;
        }

        self.saving = true;
        self.error.clear();
        let mut values = Values::new();
        values.insert("id".into(), self.api_id.clone());
        values.insert("name".into(), self.api_name.clone());
        values.insert("url".into(), api_url);
        values.insert("secret".into(), api_secret);
        values.insert("workdir".into(), working_dir);

        let task = singboard_stream::runtime().spawn(async move {
            let dir = service::data_dir()?;
            tools::run(&dir, "setup", &values, &[]).await
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(_) => {
                        this.visible = false;
                    }
                    Err(err) => {
                        this.error = err;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for SetupWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }
        if self.initial_focus {
            self.initial_focus = false;
            window.focus(&self.working_dir.focus_handle(cx), cx);
        }
        let p = Palette::get(cx);
        let scrim = if p.0 {
            rgba(0x0000007a)
        } else {
            rgba(0x00000044)
        };
        let card_bg = p.card(true);

        div()
            .id("setup-wizard-scrim").occlude()
            .absolute()
            .inset_0()
            .size_full()
            .bg(scrim)
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("setup-wizard-card").overflow_y_scroll().max_h_full().w(px(576.))
                    .max_w_full()
                    .rounded(px(18.))
                    .bg(card_bg)
                    .relative()
                    .child(crate::glass::grain_layer(px(17.)))
                    .border_1()
                    .border_color(p.rim())
                    .shadow_xl()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .text_color(p.ink())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("初始化向导"),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(p.muted())
                                    .child("只需填写工作目录，系统会自动扫描该目录及其子目录，识别 sing-box 核心与配置文件。"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.muted())
                                    .child("路径配置"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_xs().text_color(p.muted()).child("工作目录"))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div().flex_1().min_w_0().child(
                                                    crate::glass::glass_input(
                                                        "wizard-workdir-input",
                                                        &self.working_dir,
                                                        true,
                                                        p,
                                                    ),
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .id("wizard-browse")
                                                    .cursor_pointer()
                                                    .px_3()
                                                    .h(px(32.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .rounded(px(10.))
                                                    .bg(p.fill())
                                                    .hover(|s| s.bg(p.fill_strong()))
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child("浏览")
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        this.browse(window, cx);
                                                    })),
                                            ),
                                    ),
                            ),
                    )
                    .child(div().h(px(1.)).w_full().bg(p.line()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.muted())
                                    .child("Clash API"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_xs().text_color(p.muted()).child("API 地址"))
                                    .child(crate::glass::glass_input(
                                        "wizard-api-url-input",
                                        &self.api_url,
                                        true,
                                        p,
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_xs().text_color(p.muted()).child("密钥 (Secret)"))
                                    .child(crate::glass::glass_input(
                                        "wizard-api-secret-input",
                                        &self.api_secret,
                                        true,
                                        p,
                                    )),
                            ),
                    )
                    .when(!self.error.is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(p.danger())
                                .child(self.error.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .pt_1()
                            .child(
                                div()
                                    .id("wizard-go-settings")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(7.))
                                    .rounded_full()
                                    .hover(|s| s.bg(p.hover()))
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.muted())
                                    .child("前往设置页")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.go_to_settings(cx);
                                    })),
                            )
                            .child(
                                div()
                                    .id("wizard-submit")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(7.))
                                    .rounded_full()
                                    .bg(p.primary_fill())
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .child(if self.saving {
                                        "正在检测…"
                                    } else {
                                        "自动检测并继续"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save(cx);
                                    })),
                            ),
                    ).with_animation("setup-pop", Animation::new(std::time::Duration::from_millis(220)), |card,t| card.opacity((t*1.22).min(1.))),
            )
            .into_any_element()
    }
}
