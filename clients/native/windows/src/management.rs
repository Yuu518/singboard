use crate::{editor, service, theme::Palette};
use gpui::{prelude::*, *};
use gpui_component::{
    Root,
    input::{Input, InputState},
};
use service::admin::Operation;
use singboard_stream::profiles::{Profile, Store};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn store() -> Result<Store, String> {
    Ok(Store::new(
        service::data_dir()?.join("Native Subscriptions"),
    ))
}
fn core(snapshot: &service::Snapshot) -> editor::Core {
    editor::Core {
        executable: snapshot.core.clone(),
        working_dir: snapshot.working_dir.clone().into(),
    }
}

async fn check(text: &str, core: Option<&editor::Core>) -> Result<(), String> {
    if let Some(core) = core {
        let path = std::env::temp_dir().join("config.json");
        editor::validate(&path, text, Some(core)).await?;
    }
    Ok(())
}

#[derive(Default)]
struct ManagerWindow(Option<(AnyWindowHandle, WeakEntity<Manager>)>);
impl Global for ManagerWindow {}

pub fn init(cx: &mut App) {
    cx.set_global(ManagerWindow::default());
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_secs(60))
                .await;
            let result = singboard_stream::runtime()
                .spawn(async {
                    let store = store()?;
                    let due: Vec<_> = store.list()?.into_iter().filter(Profile::due).collect();
                    if due.is_empty() {
                        return Ok::<_, String>(None);
                    }
                    let current = service::snapshot()?.as_ref().map(core);
                    let mut report = Vec::new();
                    for p in due {
                        let checker = current.clone();
                        let result = store
                            .update(&p.id, None, move |text| async move {
                                check(&text, checker.as_ref()).await
                            })
                            .await;
                        report.push(format!(
                            "{} · {}",
                            p.name,
                            result.map(|_| "已更新".into()).unwrap_or_else(|e| e)
                        ));
                    }
                    Ok(Some(report.join("\n")))
                })
                .await;
            if let Ok(Ok(Some(report))) = result {
                let _ = cx.update(|cx| {
                    if let Some((_, weak)) = cx.global::<ManagerWindow>().0.clone() {
                        let _ = weak.update(cx, |view, cx| {
                            view.message = report;
                            view.refresh(cx);
                        });
                    }
                });
            }
        }
    })
    .detach();
}

#[derive(Clone)]
enum Action {
    Add,
    Edit(String),
    Save(String),
    Refresh(String),
    Open(String),
    Use(String),
    Remove(String),
    Install,
    Start,
    Stop,
    Restart,
    Uninstall,
    Task(bool),
    Sync,
    Log,
}

struct Manager {
    service: bool,
    busy: bool,
    message: String,
    pending: Option<(String, Action)>,
    status: Option<service::Status>,
    snapshot: Option<service::Snapshot>,
    profiles: Vec<Profile>,
    name: Entity<InputState>,
    url: Entity<InputState>,
    hours: Entity<InputState>,
    delay: Entity<InputState>,
    editing: Option<String>,
    executable: Option<PathBuf>,
    config: Option<PathBuf>,
    working_dir: Option<PathBuf>,
}

pub fn open(service: bool, cx: &mut App) -> Result<(), String> {
    if let Some((handle, weak)) = cx.global::<ManagerWindow>().0.clone() {
        if handle
            .update(cx, |_, window, cx| {
                window.activate_window();
                weak.update(cx, |v, cx| {
                    v.service = service;
                    v.refresh(cx);
                    cx.notify();
                })
            })
            .is_ok_and(|r| r.is_ok())
        {
            return Ok(());
        }
    }
    let bounds = Bounds::centered(None, size(px(830.), px(640.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(560.), px(400.))),
            titlebar: Some(TitlebarOptions {
                title: Some("配置与服务 · singboard".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |w, cx| {
            let host = cx.new(|cx| {
                let prefs = crate::maintenance::preferences().unwrap_or_default();
                let mut field = |placeholder, initial: &str| {
                    cx.new(|cx| {
                        InputState::new(w, cx)
                            .placeholder(placeholder)
                            .default_value(initial.to_owned())
                    })
                };
                Manager {
                    service,
                    busy: false,
                    message: String::new(),
                    pending: None,
                    status: None,
                    snapshot: None,
                    profiles: vec![],
                    name: field("名称", ""),
                    url: field("https://…", ""),
                    hours: field("小时", "0"),
                    delay: field("秒", &prefs.startup_delay_seconds.unwrap_or(0).to_string()),
                    editing: None,
                    executable: (!prefs.singbox_path.is_empty()).then(|| prefs.singbox_path.into()),
                    config: (!prefs.config_path.is_empty()).then(|| prefs.config_path.into()),
                    working_dir: (!prefs.working_dir.is_empty()).then(|| prefs.working_dir.into()),
                }
            });
            host.update(cx, |v, cx| {
                v.refresh(cx);
                crate::theme::set_caption(w, Palette::get(cx).0);
                cx.observe_global_in::<crate::theme::Appearance>(w, |_, w, cx| {
                    crate::theme::set_caption(w, Palette::get(cx).0)
                })
                .detach();
            });
            cx.global_mut::<ManagerWindow>().0 = Some((w.window_handle(), host.downgrade()));
            cx.new(|cx| Root::new(host, w, cx))
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

impl Manager {
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let job = singboard_stream::runtime().spawn_blocking(|| {
            (
                service::status(),
                service::snapshot(),
                store().and_then(|s| s.list()),
            )
        });
        cx.spawn(async move |weak, cx| {
            let result = job.await;
            let _ = weak.update(cx, |v, cx| {
                v.busy = false;
                match result {
                    Ok((status, snapshot, profiles)) => {
                        match status {
                            Ok(s) => v.status = Some(s),
                            Err(e) => {
                                v.status = None;
                                v.message = e
                            }
                        }
                        match snapshot {
                            Ok(s) => v.snapshot = s,
                            Err(e) => {
                                v.snapshot = None;
                                v.message = e
                            }
                        }
                        match profiles {
                            Ok(p) => v.profiles = p,
                            Err(e) => v.message = e,
                        }
                    }
                    Err(e) => v.message = e.to_string(),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn run(
        &mut self,
        job: impl Future<Output = Result<String, String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.message = "正在处理…".into();
        self.pending = None;
        let task = singboard_stream::runtime().spawn(job);
        cx.spawn(async move |weak, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = weak.update(cx, |v, cx| {
                v.busy = false;
                v.message = result.unwrap_or_else(|e| e);
                v.refresh(cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn pick(&mut self, target: &'static str, cx: &mut Context<Self>) {
        let directory = target == "directory";
        let pick = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple: false,
            prompt: Some(
                match target {
                    "core" => "选择 sing-box.exe",
                    "directory" => "选择核心工作目录",
                    _ => "选择源配置",
                }
                .into(),
            ),
        });
        cx.spawn(async move |weak, cx| {
            if let Ok(Ok(Some(paths))) = pick.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = weak.update(cx, |v, cx| {
                    match target {
                        "core" => v.executable = Some(path),
                        "directory" => v.working_dir = Some(path),
                        _ => v.config = Some(path),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn action(
        &mut self,
        action: Action,
        confirmed: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.busy {
            return Ok(());
        }
        let confirmation = match &action {
            Action::Install => Some("安装服务并创建登录启动任务？"),
            Action::Uninstall => Some("卸载本机服务并移除其登录启动任务？"),
            Action::Remove(_) => Some("删除此订阅及其本地备份？"),
            Action::Use(_) => Some("将服务切换到这个订阅？正在运行的核心将重启。"),
            Action::Stop => Some("停止本机服务？"),
            Action::Restart => Some("使用当前源配置重启服务？"),
            _ => None,
        };
        if !confirmed && let Some(message) = confirmation {
            self.pending = Some((message.into(), action));
            cx.notify();
            return Ok(());
        }
        self.pending = None;
        let store = store()?;
        let checker = self.snapshot.as_ref().map(core);
        let service = self
            .status
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| singboard_service::SERVICE_NAME.into());
        let operation = match action {
            Action::Edit(id) => {
                let p = self
                    .profiles
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or("订阅不存在")?;
                for (field, value) in [
                    (&self.name, p.name.clone()),
                    (&self.url, p.url.clone()),
                    (&self.hours, p.hours.to_string()),
                ] {
                    field.update(cx, |f, cx| f.set_value(value, w, cx));
                }
                self.editing = Some(id);
                cx.notify();
                return Ok(());
            }
            Action::Add | Action::Save(_) => {
                let name = self.name.read(cx).value().to_string();
                let url = self.url.read(cx).value().to_string();
                let hours = self
                    .hours
                    .read(cx)
                    .value()
                    .trim()
                    .parse()
                    .map_err(|_| "更新间隔需要填写 0–576 的整数")?;
                self.run(
                    async move {
                        let validate =
                            move |text: String| async move { check(&text, checker.as_ref()).await };
                        if let Action::Save(id) = action {
                            store
                                .update(&id, Some((&name, &url, hours)), validate)
                                .await?;
                        } else {
                            store.add(&name, &url, hours, validate).await?;
                        }
                        Ok("订阅已保存".into())
                    },
                    cx,
                );
                return Ok(());
            }
            Action::Refresh(id) => {
                self.run(
                    async move {
                        let changed = store
                            .update(&id, None, move |text| async move {
                                check(&text, checker.as_ref()).await
                            })
                            .await?;
                        Ok(if changed {
                            "已更新配置；服务使用新内容需重启"
                        } else {
                            "已是最新内容"
                        }
                        .into())
                    },
                    cx,
                );
                return Ok(());
            }
            Action::Open(id) => return editor::open(store.path(&id)?, checker, cx),
            Action::Remove(id) => {
                let path = store.path(&id)?;
                self.run(
                    async move {
                        if service::snapshot()?.is_some_and(|s| same_file(&s.source_config, &path))
                        {
                            return Err("请先将服务切换到其他配置".into());
                        }
                        store.remove(&id)?;
                        Ok("订阅已删除".into())
                    },
                    cx,
                );
                return Ok(());
            }
            Action::Use(id) => {
                let path = store.path(&id)?;
                if self.snapshot.is_none() {
                    self.config = Some(path);
                    self.service = true;
                    cx.notify();
                    return Ok(());
                }
                self.run(
                    async move {
                        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                        check(&text, checker.as_ref()).await?;
                        service::request(Operation::UseConfig {
                            service,
                            config: path,
                        })
                        .await?;
                        Ok("服务已切换到订阅配置".into())
                    },
                    cx,
                );
                return Ok(());
            }
            Action::Install => {
                let core = self.executable.clone().ok_or("请先选择核心")?;
                let config = self.config.clone().ok_or("请先选择源配置")?;
                let working_dir = self
                    .working_dir
                    .clone()
                    .unwrap_or_else(|| core.parent().unwrap_or(Path::new(".")).to_owned());
                let delay = self.delay_seconds(cx)?;
                self.run(
                    async move {
                        let checker = editor::Core {
                            executable: core.clone(),
                            working_dir: working_dir.clone(),
                        };
                        let text = std::fs::read_to_string(&config).map_err(|e| e.to_string())?;
                        check(&text, Some(&checker)).await?;
                        service::request(Operation::Install {
                            service: singboard_service::SERVICE_NAME.into(),
                            core,
                            config,
                            working_dir,
                            delay,
                        })
                        .await?;
                        Ok("服务已安装".into())
                    },
                    cx,
                );
                return Ok(());
            }
            Action::Start => Operation::Start { service },
            Action::Stop => Operation::Stop { service },
            Action::Restart => Operation::Restart { service },
            Action::Uninstall => Operation::Uninstall { service },
            Action::Task(true) => Operation::CreateTask {
                service,
                delay: self.delay_seconds(cx)?,
            },
            Action::Task(false) => Operation::DeleteTask { service },
            Action::Sync if service != singboard_service::SERVICE_NAME => {
                Operation::Migrate { service }
            }
            Action::Sync => Operation::Sync { service },
            Action::Log => {
                self.run(
                    async {
                        let log = service::error_log()?;
                        Ok(if log.trim().is_empty() {
                            "暂无服务错误日志".into()
                        } else {
                            log
                        })
                    },
                    cx,
                );
                return Ok(());
            }
        };
        self.run(
            async move {
                let check = matches!(
                    operation,
                    Operation::Start { .. } | Operation::Restart { .. }
                );
                service::request(operation).await?;
                if check {
                    Ok(
                        singboard_stream::tools::readiness(&crate::maintenance::preferences()?)
                            .await,
                    )
                } else {
                    Ok("操作完成".into())
                }
            },
            cx,
        );
        Ok(())
    }

    fn delay_seconds(&self, cx: &App) -> Result<u32, String> {
        self.delay
            .read(cx)
            .value()
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|s| *s <= 3600)
            .ok_or_else(|| "启动延迟应为 0–3600 秒".into())
    }

    fn button(
        &self,
        id: impl Into<ElementId>,
        label: &'static str,
        action: Action,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        chip(id, label, Palette::get(cx)).when(!self.busy, |v| {
            v.on_click(cx.listener(move |this, _, w, cx| {
                if let Err(e) = this.action(action.clone(), false, w, cx) {
                    this.message = e;
                    cx.notify();
                }
            }))
        })
    }

    fn service_page(&self, cx: &Context<Self>) -> Div {
        let palette = Palette::get(cx);
        let mut page = div().flex().flex_col().gap_3();
        let Some(status) = &self.status else {
            return page.child("正在读取服务状态");
        };
        let state = match status.state.as_str() {
            "running" => "运行中",
            "stopped" => "已停止",
            "starting" => "正在启动",
            "stopping" => "正在停止",
            "not_installed" => "未安装",
            _ => "未知",
        };
        page = page.child(format!(
            "{} · {state}{}",
            status.name,
            status
                .pid
                .map(|p| format!(" · PID {p}"))
                .unwrap_or_default()
        ));
        if status.state == "not_installed" {
            for (id, label, path) in [
                ("core", "选择核心", self.executable.as_ref()),
                ("config", "选择配置", self.config.as_ref()),
                ("directory", "选择工作目录", self.working_dir.as_ref()),
            ] {
                page = page.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            chip(id, label, palette)
                                .on_click(cx.listener(move |v, _, _, cx| v.pick(id, cx))),
                        )
                        .child(path.map(|p| p.display().to_string()).unwrap_or_default()),
                );
            }
            return page
                .child(field("登录启动延迟（秒）", &self.delay))
                .child(self.button("install", "安装服务", Action::Install, cx));
        }
        let mut controls = div().flex().flex_wrap().gap_2();
        if status.state == "running" {
            controls = controls
                .child(self.button("stop", "停止", Action::Stop, cx))
                .child(self.button("restart", "重启", Action::Restart, cx));
        }
        if status.state == "stopped" {
            controls = controls.child(self.button("start", "启动", Action::Start, cx));
        }
        page = page.child(
            controls
                .child(self.button("sync", "同步服务组件", Action::Sync, cx))
                .child(self.button("log", "错误日志", Action::Log, cx))
                .child(self.button("uninstall", "卸载", Action::Uninstall, cx)),
        );
        if let Some(s) = &self.snapshot {
            page = page
                .child(format!("源配置 · {}", s.source_config.display()))
                .child(format!("工作目录 · {}", s.working_dir));
        }
        page.child(field("登录启动延迟（秒）", &self.delay)).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(self.button("task", "保存登录启动任务", Action::Task(true), cx))
                .when(status.autostart, |v| {
                    v.child(self.button("disable", "取消登录启动", Action::Task(false), cx))
                })
                .child(if status.autostart {
                    "已启用"
                } else {
                    "已关闭"
                }),
        )
    }

    fn profile_page(&self, cx: &Context<Self>) -> Div {
        let palette = Palette::get(cx);
        let mut page = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(field("订阅名称", &self.name))
            .child(field("订阅地址", &self.url))
            .child(field("自动更新间隔（小时，0 为关闭）", &self.hours));
        if let Some(id) = &self.editing {
            page = page.child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.button("save", "保存订阅", Action::Save(id.clone()), cx))
                    .child(
                        chip("cancel-edit", "结束编辑", palette).on_click(cx.listener(
                            |v, _, w, cx| {
                                v.editing = None;
                                for (input, text) in [(&v.name, ""), (&v.url, ""), (&v.hours, "0")]
                                {
                                    input.update(cx, |f, cx| f.set_value(text, w, cx));
                                }
                                cx.notify();
                            },
                        )),
                    ),
            );
        } else {
            page = page.child(self.button("add", "添加订阅", Action::Add, cx));
        }
        for (i, p) in self.profiles.iter().enumerate() {
            let mut bar = div().flex().flex_wrap().gap_2();
            for (n, (label, action)) in [
                ("更新", Action::Refresh(p.id.clone())),
                ("编辑内容", Action::Open(p.id.clone())),
                ("订阅设置", Action::Edit(p.id.clone())),
                ("用于服务", Action::Use(p.id.clone())),
                ("删除", Action::Remove(p.id.clone())),
            ]
            .into_iter()
            .enumerate()
            {
                bar = bar.child(self.button(("profile-action", i * 5 + n), label, action, cx));
            }
            let period = if p.hours == 0 {
                "手动更新".to_owned()
            } else {
                format!("每 {} 小时更新", p.hours)
            };
            page = page.child(
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(palette.card(false))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(format!("{} · {period}", p.name))
                    .child(bar),
            );
        }
        page
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(),b.canonicalize()),(Ok(a),Ok(b)) if a==b)
}
fn chip(id: impl Into<ElementId>, label: &'static str, palette: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .rounded_md()
        .px_3()
        .py_1()
        .bg(palette.fill())
        .text_sm()
        .child(label)
}
fn field(label: &'static str, input: &Entity<InputState>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_sm().child(label))
        .child(Input::new(input))
}

impl Render for Manager {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::get(cx);
        let mut controls = div().flex().items_center().gap_2();
        for (i, (label, service)) in [("远程订阅", false), ("Windows 服务", true)]
            .into_iter()
            .enumerate()
        {
            controls = controls.child(
                chip(("tab", i), label, palette)
                    .when(service == self.service, |v| v.text_color(palette.accent()))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.service = service;
                        v.pending = None;
                        v.refresh(cx);
                        cx.notify();
                    })),
            );
        }
        controls = controls.child(
            chip("refresh", "刷新", palette).on_click(cx.listener(|v, _, _, cx| v.refresh(cx))),
        );
        let mut body = div()
            .size_full()
            .flex()
            .flex_col()
            .p_4()
            .gap_3()
            .bg(palette.page())
            .text_color(palette.ink())
            .child(controls);
        if let Some((question, action)) = self.pending.clone() {
            body = body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(question)
                    .child(chip("confirm", "确认", palette).on_click(cx.listener(
                        move |v, _, w, cx| {
                            if let Err(e) = v.action(action.clone(), true, w, cx) {
                                v.message = e;
                                cx.notify();
                            }
                        },
                    )))
                    .child(
                        chip("cancel", "取消", palette).on_click(cx.listener(|v, _, _, cx| {
                            v.pending = None;
                            cx.notify();
                        })),
                    ),
            );
        }
        body.child(
            div()
                .id("manager-message")
                .max_h(px(96.))
                .overflow_y_scroll()
                .text_sm()
                .child(self.message.clone()),
        )
        .child(
            div()
                .id("manager-page")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(if self.service {
                    self.service_page(cx)
                } else {
                    self.profile_page(cx)
                }),
        )
    }
}
