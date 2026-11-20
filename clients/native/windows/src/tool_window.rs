use crate::{core_actions, maintenance, service, theme::Palette, tools};
use gpui::{prelude::*, *};
use gpui_component::{
    Root,
    input::{Input, InputState},
};
use tools::{Form, Reply, Values};

#[derive(Default)]
struct OpenTools(Option<(AnyWindowHandle, WeakEntity<ToolWindow>)>);
impl Global for OpenTools {}
pub fn init(cx: &mut App) {
    cx.set_global(OpenTools::default());
}
struct ToolWindow {
    forms: Vec<Form>,
    index: usize,
    inputs: Vec<Entity<InputState>>,
    message: String,
    hide_result: bool,
    busy: bool,
    pending: Option<(&'static str, Values, Option<String>)>,
}
fn catalog() -> Result<Vec<Form>, String> {
    let mut prefs = maintenance::preferences()?;
    if let Ok(Some(s)) = service::snapshot() {
        prefs.config_path = s.source_config.to_string_lossy().into();
        prefs.singbox_path = s.source_core.to_string_lossy().into();
        prefs.working_dir = s.working_dir;
    }
    Ok(tools::forms(&prefs))
}
pub fn open(index: usize, cx: &mut App) -> Result<(), String> {
    if let Some((window, view)) = cx.global::<OpenTools>().0.clone()
        && window
            .update(cx, |_, w, cx| {
                w.activate_window();
                view.update(cx, |v, cx| {
                    if !v.busy {
                        v.select(index, w, cx);
                    }
                })
            })
            .is_ok_and(|r| r.is_ok())
    {
        return Ok(());
    }
    let forms = catalog()?;
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(720.), px(610.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("面板设置与工具".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |w, cx| {
            let view = cx.new(|cx| {
                let mut v = ToolWindow {
                    forms,
                    index: 0,
                    inputs: vec![],
                    message: String::new(),
                    hide_result: false,
                    busy: false,
                    pending: None,
                };
                v.select(index, w, cx);
                v
            });
            cx.global_mut::<OpenTools>().0 = Some((w.window_handle(), view.downgrade()));
            cx.new(|cx| Root::new(view, w, cx))
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn rule(tag: String, query: String, cx: &mut App) -> Result<(), String> {
    open(5, cx)?;
    if let Some((handle, view)) = cx.global::<OpenTools>().0.clone() {
        let _ = handle.update(cx, |_, w, cx| {
            view.update(cx, |v, cx| {
                if v.busy {
                    return;
                }
                for (key, value) in [("tag", tag), ("query", query)] {
                    if let Some(index) = v.forms[v.index].fields.iter().position(|f| f.key == key) {
                        v.inputs[index].update(cx, |input, cx| input.set_value(value, w, cx));
                    }
                }
                cx.notify();
            })
        });
    }
    Ok(())
}
fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(p.fill())
        .child(label.into())
}
impl ToolWindow {
    fn select(&mut self, i: usize, w: &mut Window, cx: &mut Context<Self>) {
        self.index = i.min(self.forms.len() - 1);
        self.message.clear();
        self.pending = None;
        self.inputs = self.forms[self.index]
            .fields
            .iter()
            .map(|f| {
                cx.new(|cx| {
                    InputState::new(w, cx)
                        .default_value(f.value.clone())
                        .masked(f.secret)
                })
            })
            .collect();
        cx.notify();
    }
    fn submit(
        &mut self,
        action: &'static str,
        values: Values,
        ticket: Option<String>,
        confirmed: bool,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if tools::confirm(action) && !confirmed {
            self.message = "确认执行此操作？".into();
            self.pending = Some((action, values, None));
            cx.notify();
            return;
        }
        self.pending = None;
        self.busy = true;
        self.message = "正在处理…".into();
        if ticket.is_some() {
            cx.spawn(async move |weak, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(400))
                        .await;
                    if weak
                        .update(cx, |v, cx| {
                            if v.busy {
                                v.message = core_actions::progress();
                                cx.notify();
                            }
                            v.busy
                        })
                        .ok()
                        != Some(true)
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let request = values.clone();
        let task = singboard_stream::runtime().spawn(async move {
            let dir = service::data_dir()?;
            if action.starts_with("core_") {
                core_actions::run(&dir, action, &request, ticket.as_deref()).await
            } else if action == "local_use" {
                let id = tools::value(&request, "profile");
                let prefs = maintenance::preferences()?;
                let profile = prefs
                    .config_profiles
                    .iter()
                    .find(|v| v.kind == "local" && v.id == id)
                    .ok_or("请选择本地配置")?;
                let core = tools::value(&request, "core");
                let work = tools::value(&request, "workdir");
                singboard_stream::config_files::run_singbox_check(
                    core,
                    &profile.source,
                    (!work.is_empty()).then_some(work),
                )
                .await?;
                if service::snapshot()?.is_some() {
                    let service = service::service_name()?;
                    service::request(service::admin::Operation::UseConfig {
                        service,
                        config: profile.source.clone().into(),
                    })
                    .await?;
                }
                crate::settings::Settings::edit(&dir, |p| {
                    p.active_config_profile_id = id.into();
                    p.config_path = profile.source.clone();
                    Ok(())
                })?;
                Ok(Reply::saved("已切换配置，运行中的核心可在服务管理中重启"))
            } else {
                tools::run(&dir, action, &request, &[]).await
            }
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = weak.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Err(e) => this.message = e,
                    Ok(Reply {
                        text,
                        ticket,
                        refresh,
                    }) => {
                        this.pending = ticket.map(|t| (action, values, Some(t)));
                        this.message = text;
                        if refresh && let Ok(forms) = catalog() {
                            this.forms = forms;
                            if let Some((handle, _)) = cx.global::<OpenTools>().0.clone() {
                                let idx = this.index;
                                let msg = this.message.clone();
                                let weak = cx.weak_entity();
                                cx.defer(move |cx| {
                                    let _ = handle.update(cx, |_, w, cx| {
                                        weak.update(cx, |v, cx| {
                                            v.select(idx, w, cx);
                                            v.message = msg;
                                        })
                                    });
                                });
                            }
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
impl Render for ToolWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let mut tabs = div().flex().flex_wrap().gap_2();
        for (i, form) in self.forms.iter().enumerate() {
            tabs = tabs.child(
                button(("tool", i), form.title, p)
                    .when(i == self.index, |b| b.text_color(p.accent()))
                    .on_click(cx.listener(move |v, _, w, cx| {
                        if !v.busy {
                            v.select(i, w, cx);
                        }
                    })),
            );
        }
        let mut page = div()
            .id("tool-fields")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2();
        let form = &self.forms[self.index];
        for (i, f) in form.fields.iter().enumerate() {
            if f.label.is_empty() {
                continue;
            }
            page = page.child(f.label);
            if f.choices.is_empty() {
                page = page.child(Input::new(&self.inputs[i]));
            } else {
                let mut choices = div().flex().flex_wrap().gap_2();
                let selected = self.inputs[i].read(cx).value();
                for (j, (key, name)) in f.choices.iter().enumerate() {
                    let key = key.clone();
                    choices = choices.child(
                        button(("choice", i * 10000 + j), name.clone(), p)
                            .when(selected.as_str() == key, |b| b.text_color(p.accent()))
                            .on_click(cx.listener(move |v, _, w, cx| {
                                if !v.busy {
                                    v.inputs[i].update(cx, |f, cx| f.set_value(key.clone(), w, cx));
                                    v.pending = None;
                                    cx.notify();
                                }
                            })),
                    );
                }
                page = page.child(choices);
            }
        }
        let mut actions = div().flex().flex_wrap().gap_2();
        if let Some((action, values, ticket)) = self.pending.clone() {
            actions = actions
                .child(button("confirm-tool", "确认", p).on_click(cx.listener(
                    move |v, _, _, cx| v.submit(action, values.clone(), ticket.clone(), true, cx),
                )))
                .child(
                    button("cancel-tool", "取消", p).on_click(cx.listener(|v, _, _, cx| {
                        v.pending = None;
                        v.message.clear();
                        cx.notify();
                    })),
                );
        } else {
            for &(action, label) in &form.actions {
                actions = actions.child(button(action, label, p).on_click(cx.listener(
                    move |v, _, _, cx| {
                        let values = v.forms[v.index]
                            .fields
                            .iter()
                            .zip(&v.inputs)
                            .map(|(f, e)| (f.key.into(), e.read(cx).value().to_string()))
                            .collect();
                        v.submit(action, values, None, false, cx);
                    },
                )));
            }
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .text_sm()
            .bg(p.page())
            .text_color(p.ink())
            .child(tabs)
            .child(
                page.child(actions)
                    .when(self.index == 3 && !self.message.is_empty(), |d| {
                        d.child(
                            button(
                                "hide-result",
                                if self.hide_result {
                                    "显示结果"
                                } else {
                                    "隐藏结果"
                                },
                                p,
                            )
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.hide_result = !v.hide_result;
                                cx.notify();
                            })),
                        )
                    })
                    .when(self.index != 3 || !self.hide_result, |d| {
                        d.child(self.message.clone())
                    }),
            )
    }
}
