use super::*;
use std::path::{Path, PathBuf};

fn host_for_display(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str().map(|h| match u.port() {
                Some(p) => format!("{h}:{p}"),
                None => h.to_owned(),
            })
        })
        .unwrap_or_else(|| url.to_owned())
}

fn validate_github_token(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        return None;
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Some("Token 只能包含字母、数字和下划线，请检查是否多复制了空格或引号");
    }
    if value.len() < 20 || value.len() > 255 {
        return Some("Token 长度不对，请重新复制完整的 Token");
    }
    None
}

fn active_config_file(prefs: &Settings) -> Result<PathBuf, String> {
    if let Some(p) = prefs
        .config_profiles
        .iter()
        .find(|p| p.id == prefs.active_config_profile_id)
    {
        if p.kind == "remote" {
            let path = crate::service::data_dir()?
                .join("remote-configs")
                .join(format!("{}.json", p.id));
            if path.is_file() {
                return Ok(path);
            }
        } else if !p.source.trim().is_empty() {
            let path = PathBuf::from(p.source.trim());
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    if !prefs.config_path.trim().is_empty() {
        let path = PathBuf::from(prefs.config_path.trim());
        if path.is_file() {
            return Ok(path);
        }
    }
    Err("请先在配置页选择有效的配置文件".into())
}

impl Workspace {
    pub(super) fn edit_preferences(
        &mut self,
        change: impl FnOnce(&mut Settings) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = crate::service::data_dir().and_then(|dir| Settings::edit(&dir, change)) {
            self.message(e,true,cx);
        }
        cx.notify();
    }

    fn field(
        &mut self,
        key: &'static str,
        value: &str,
        hint: &'static str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let (input, baseline) = self.settings_layout.fields.entry(key).or_insert_with(|| {
            (
                cx.new(|cx| {
                    InputState::new(w, cx)
                        .default_value(value.to_string())
                        .placeholder(hint)
                        .masked(key == "secret")
                }),
                value.to_string(),
            )
        });
        if baseline != value {
            if input.read(cx).value().as_str() == baseline {
                input.update(cx, |i, cx| i.set_value(value.to_string(), w, cx));
            }
            *baseline = value.to_string();
        }
        input.clone()
    }

    pub(super) fn disclosure(
        &self,
        id: &'static str,
        label: &'static str,
        summary: String,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let p = Palette::get(cx);
        let open = self.settings_layout.open.contains(id);
        div()
            .id(id)
            .cursor_pointer()
            .w_full()
            .min_h(px(56.))
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .hover(|s| s.bg(p.hover()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(crate::theme::text_px(13.)).line_height(crate::theme::text_px(18.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .line_height(crate::theme::text_px(17.))
                            .text_color(p.muted())
                            .child(summary),
                    ),
            )
            .child(
                svg().path("chevron-down").size(px(16.)).text_color(p.muted())
                    .with_transformation(Transformation::rotate(percentage(if open { 0.5 } else { 0. }))),
            )
            .on_click(cx.listener(move |v, _, _, cx| {
                if !v.settings_layout.open.remove(id) {
                    v.settings_layout.open.insert(id);
                }
                cx.notify();
            }))
    }

    pub(super) fn result(&self, section: &'static str, p: Palette) -> Div {
        div().when_some(self.settings_layout.results.get(section), |el, text| {
            el.px_4()
                .pb_3()
                .text_xs()
                .text_color(p.muted())
                .child(text.clone())
        })
    }

    pub(super) fn install_windows_service(&mut self, cx: &mut Context<Self>) {
        if !self.settings_layout.pending.insert("service-op") {
            return;
        }
        self.settings_layout.service_action = Some("install");
        self.settings_layout
            .results
            .insert("service-op", "正在安装 Windows 服务…".into());
        let job = singboard_stream::runtime().spawn(async move {
            let prefs = crate::maintenance::preferences()?;
            let core_str = prefs.singbox_path.trim().to_owned();
            if core_str.is_empty() {
                return Err("请先在「服务参数」中设置 sing-box 可执行文件路径".into());
            }
            let core = PathBuf::from(&core_str);
            if !core.is_file() {
                return Err(format!("找不到 sing-box 可执行文件：{core_str}"));
            }
            let config = active_config_file(&prefs)?;
            let working_dir = if prefs.working_dir.trim().is_empty() {
                core.parent().unwrap_or(Path::new(".")).to_path_buf()
            } else {
                PathBuf::from(prefs.working_dir.trim())
            };
            let delay = prefs.startup_delay_seconds.unwrap_or(30).min(3600);
            let text = std::fs::read_to_string(&config).map_err(|e| e.to_string())?;
            singboard_stream::profiles::validate_jsonc(&text)?;
            singboard_stream::config_files::run_singbox_check(
                &core_str,
                &config.to_string_lossy(),
                Some(&working_dir.to_string_lossy()),
            )
            .await?;
            crate::service::request(crate::service::admin::Operation::Install {
                service: singboard_service::SERVICE_NAME.into(),
                core,
                config,
                working_dir,
                delay,
            })
            .await?;
            Ok::<_, String>("Windows 服务已安装".into())
        });
        cx.spawn(async move |weak, cx| {
            let r = job.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = weak.update(cx, |v, cx| {
                v.settings_layout.service_action = None;
                v.settings_layout.pending.remove("service-op");
                if r.is_ok() {
                    v.refresh_local_state(cx);
                }
                v.settings_layout
                    .results
                    .insert("service-op", r.unwrap_or_else(|e| e));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn run_setting(
        &mut self,
        section: &'static str,
        action: &'static str,
        values: Values,
        cx: &mut Context<Self>,
    ) {
        if !self.settings_layout.pending.insert(section) {
            return;
        }
        if action == "uninstall_service" {
            self.settings_layout.service_action = Some("uninstall");
        }
        self.settings_layout
            .results
            .insert(section, "正在处理…".into());
        let values_copy = values.clone();
        let job = singboard_stream::runtime().spawn(async move {
            let dir = crate::service::data_dir()?;
            match action {
                "core_check" => crate::core_actions::run(&dir, "core_prepare", &values, None).await,
                "uninstall_service" => {
                    let service = crate::service::service_name()?;
                    crate::service::request(crate::service::admin::Operation::Uninstall {
                        service,
                    })
                    .await?;
                    Ok(singboard_stream::tools::Reply::text("服务已卸载"))
                }
                _ => singboard_stream::tools::run(&dir, action, &values, &[]).await,
            }
        });
        cx.spawn(async move |weak, cx| {
            let r = job.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = weak.update(cx, |v, cx| {
                if action == "uninstall_service" {
                    v.settings_layout.service_action = None;
                }
                v.settings_layout.pending.remove(section);
                if r.is_ok() && action == "uninstall_service" {
                    v.refresh_local_state(cx);
                }
                if r.is_ok()
                    && matches!(action, "save_api" | "add_api" | "delete_api" | "switch_api")
                {
                    v.settings_layout.api_edit = None;
                }
                match r {
                    Ok(reply) => {
                        if action == "core_check"
                            && let Some(ticket) = reply.ticket
                        {
                            let mut lines = reply.text.lines();
                            let summary = lines.next().unwrap_or("").to_owned();
                            let target = lines
                                .next()
                                .unwrap_or("")
                                .strip_prefix("更新目标：")
                                .unwrap_or("")
                                .to_owned();
                            v.settings_layout
                                .results
                                .insert(section, format!("可用版本 {summary}"));
                            v.settings_layout.dialog = Some(SettingsDialog::ConfirmCoreUpdate {
                                summary,
                                target,
                                ticket,
                                values: values_copy,
                                busy: false,
                                error: String::new(),
                            });
                        } else {
                            v.settings_layout.results.insert(section, reply.text);
                        }
                    }
                    Err(e) => {
                        v.settings_layout.results.insert(section, e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn run_dns_query(
        &mut self,
        domain: String,
        record: String,
        resolver: String,
        custom_server: String,
        geo_enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.settings_layout.pending.insert("dns") {
            return;
        }
        let domain = domain.trim().to_owned();
        if domain.is_empty() {
            self.settings_layout.pending.remove("dns");
            self.settings_layout
                .results
                .insert("dns", "请输入域名。".into());
            cx.notify();
            return;
        }
        let server = match resolver.as_str() {
            "system" | "" => None,
            "custom" => {
                let s = custom_server.trim().to_owned();
                if s.is_empty() {
                    self.settings_layout.pending.remove("dns");
                    self.settings_layout
                        .results
                        .insert("dns", "请输入自定义解析器地址。".into());
                    cx.notify();
                    return;
                }
                Some(s)
            }
            s => Some(s.to_owned()),
        };
        self.settings_layout.results.remove("dns");
        self.settings_layout.dns_view = None;
        self.settings_layout.dns_history_open = false;
        let job = singboard_stream::runtime().spawn(async move {
            let val = singboard_stream::tools::dns_records(
                &crate::service::data_dir()?,
                domain,
                record,
                server.clone(),
                &[],
                geo_enabled,
            )
            .await?;
            let source_label = if val["source"].as_str() == Some("custom") {
                format!(
                    "指定 DNS: {}",
                    val["server"]
                        .as_str()
                        .or(server.as_deref())
                        .unwrap_or("自定义")
                )
            } else {
                "本机当前 DNS".into()
            };
            let mut rows = Vec::new();
            for rec in val["records"].as_array().into_iter().flatten() {
                rows.push(DnsRow {
                    record_type: rec["record_type"].as_str().unwrap_or("").to_owned(),
                    name: rec["name"].as_str().unwrap_or("").to_owned(),
                    ttl: rec["ttl"].as_u64().unwrap_or(0) as u32,
                    value: rec["value"].as_str().unwrap_or("").to_owned(),
                    geo: rec["geo"].as_str().unwrap_or("").to_owned(),
                });
            }
            Ok::<_, String>(DnsView {
                source_label,
                records: rows,
            })
        });
        cx.spawn(async move |weak, cx| {
            let r = job.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = weak.update(cx, |v, cx| {
                v.settings_layout.pending.remove("dns");
                match r {
                    Ok(view) => {
                        v.settings_layout.dns_view = Some(view);
                        v.settings_layout.results.remove("dns");
                    }
                    Err(e) => {
                        v.settings_layout.results.insert("dns", e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn values(&self, keys: &[&str], cx: &App) -> Values {
        keys.iter()
            .filter_map(|&k| {
                self.settings_layout
                    .fields
                    .get(k)
                    .map(|(i, _)| (k.to_owned(), i.read(cx).value().to_string()))
            })
            .collect()
    }

    pub(super) fn service_parameters(
        &mut self,
        prefs: &Settings,
        narrow: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = Palette::get(cx);
        let delay = prefs.startup_delay_seconds.unwrap_or(30).to_string();
        let delay_input = self.field("delay", &delay, "30", w, cx);
        let core_input = self.field(
            "core",
            &prefs.singbox_path,
            "C:\\sing-box\\sing-box.exe",
            w,
            cx,
        );
        let workdir_input = self.field(
            "workdir",
            &prefs.working_dir,
            "留空则使用配置文件所在目录",
            w,
            cx,
        );
        let busy = self.settings_layout.pending.contains("service");

        let path_row =
            |id_btn: &'static str, label: &'static str, input: Entity<InputState>, files: bool| {
                div()
                    .flex()
                    .gap_x_4()
                    .gap_y_2()
                    .when(narrow, |s| s.flex_col())
                    .when(!narrow, |s| s.items_center())
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted())
                            .when(!narrow, |s| s.w(px(150.)).flex_none())
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(glass_input(SharedString::from(format!("svc-path-input-{id_btn}")), &input, true, p)))
                            .child(action_btn(id_btn, "浏览", false, p).on_click(cx.listener(
                                move |_, _, window, cx| {
                                    let handle = window.window_handle();
                                    let pick = cx.prompt_for_paths(PathPromptOptions {
                                        files,
                                        directories: !files,
                                        multiple: false,
                                        prompt: Some(label.into()),
                                    });
                                    let input = input.clone();
                                    cx.spawn(async move |_, cx| {
                                        if let Ok(Ok(Some(paths))) = pick.await
                                            && let Some(path) = paths.first()
                                        {
                                            let _ = handle.update(cx, |_, w, cx| {
                                                input.update(cx, |i, cx| {
                                                    i.set_value(path.display().to_string(), w, cx)
                                                })
                                            });
                                        }
                                    })
                                    .detach();
                                },
                            ))),
                    )
            };

        div()
            .px_4()
            .pb_3()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .gap_x_4()
                    .gap_y_2()
                    .when(narrow, |s| s.flex_col())
                    .when(!narrow, |s| s.items_start())
                    .child(
                        div()
                            .when(!narrow, |s| s.w(px(150.)).flex_none().pt_1())
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("登录后延迟启动"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted())
                                    .child("登录 Windows 后等这么久再启动核心服务。"),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(140.))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(glass_input("svc-delay-input", &delay_input, true, p)),
                                    )
                                    .child(div().text_xs().text_color(p.muted()).child("秒")),
                            )
                            .when(!self.local_state.contains("未安装"), |row| {
                                row.child(
                                    action_btn("enable-svc-autostart", "启用自启", busy, p)
                                        .when(!busy, |b| {
                                            b.on_click(cx.listener(|v, _, _, cx| {
                                                let values = v.values(&["delay"], cx);
                                                let delay = values["delay"]
                                                    .trim()
                                                    .parse::<u32>()
                                                    .unwrap_or(30)
                                                    .min(3600);
                                                v.settings_layout.pending.insert("service");
                                                let job = singboard_stream::runtime().spawn(async move {
                                                    let service = crate::service::service_name()?;
                                                    crate::service::request(
                                                        crate::service::admin::Operation::CreateTask {
                                                            service,
                                                            delay,
                                                        },
                                                    )
                                                    .await
                                                });
                                                cx.spawn(async move |weak, cx| {
                                                    let r = job
                                                        .await
                                                        .map_err(|e| e.to_string())
                                                        .and_then(|r| r);
                                                    let _ = weak.update(cx, |v, cx| {
                                                        v.settings_layout.pending.remove("service");
                                                        v.settings_layout.results.insert(
                                                            "service",
                                                            r.map(|()| "已启用服务开机自启".to_string())
                                                                .unwrap_or_else(|e| e),
                                                        );
                                                        cx.notify();
                                                    });
                                                })
                                                .detach();
                                                cx.notify();
                                            }))
                                        }),
                                )
                                .child(
                                    action_btn("disable-svc-autostart", "取消自启", busy, p)
                                        .when(!busy, |b| {
                                            b.on_click(cx.listener(|v, _, _, cx| {
                                                v.settings_layout.pending.insert("service");
                                                let job = singboard_stream::runtime().spawn(async move {
                                                    let service = crate::service::service_name()?;
                                                    crate::service::request(
                                                        crate::service::admin::Operation::DeleteTask {
                                                            service,
                                                        },
                                                    )
                                                    .await
                                                });
                                                cx.spawn(async move |weak, cx| {
                                                    let r = job
                                                        .await
                                                        .map_err(|e| e.to_string())
                                                        .and_then(|r| r);
                                                    let _ = weak.update(cx, |v, cx| {
                                                        v.settings_layout.pending.remove("service");
                                                        v.settings_layout.results.insert(
                                                            "service",
                                                            r.map(|()| "已取消服务开机自启".to_string())
                                                                .unwrap_or_else(|e| e),
                                                        );
                                                        cx.notify();
                                                    });
                                                })
                                                .detach();
                                                cx.notify();
                                            }))
                                        }),
                                )
                            }),
                    ),
            )
            .child(path_row(
                "browse-core",
                "sing-box 可执行文件",
                core_input,
                true,
            ))
            .child(path_row("browse-workdir", "工作目录", workdir_input, false))
            .child(
                div().flex().justify_end().child(
                    action_btn(
                        "save-service-parameters",
                        if busy { "保存中…" } else { "保存参数" },
                        busy,
                        p,
                    )
                    .when(!busy, |b| {
                        b.on_click(cx.listener(|v, _, _, cx| {
                            let values = v.values(&["delay", "core", "workdir"], cx);
                            let Ok(delay) = values["delay"].trim().parse::<u32>() else {
                                v.settings_layout
                                    .results
                                    .insert("service", "启动延迟须为 0 到 3600 秒".into());
                                cx.notify();
                                return;
                            };
                            if delay > 3600 {
                                v.settings_layout
                                    .results
                                    .insert("service", "启动延迟须为 0 到 3600 秒".into());
                                cx.notify();
                                return;
                            }
                            v.settings_layout.pending.insert("service");
                            let job = singboard_stream::runtime().spawn(async move {
                                let dir = crate::service::data_dir()?;
                                if Settings::load(&dir)?.startup_delay_seconds.unwrap_or(30) != delay
                                {
                                    let status = crate::service::status()?;
                                    if status.autostart {
                                        crate::service::request(
                                            crate::service::admin::Operation::CreateTask {
                                                service: status.name,
                                                delay,
                                            },
                                        )
                                        .await?;
                                    }
                                }
                                Settings::edit(&dir, |p| {
                                    p.startup_delay_seconds = Some(delay);
                                    p.singbox_path = values["core"].trim().into();
                                    p.working_dir = values["workdir"].trim().into();
                                    Ok(())
                                })
                            });
                            cx.spawn(async move |weak, cx| {
                                let r = job.await.map_err(|e| e.to_string()).and_then(|r| r);
                                let _ = weak.update(cx, |v, cx| {
                                    v.settings_layout.pending.remove("service");
                                    v.settings_layout.results.insert(
                                        "service",
                                        r.map(|()| "服务参数已保存".to_string())
                                            .unwrap_or_else(|e| e),
                                    );
                                    cx.notify();
                                });
                            })
                            .detach();
                            cx.notify();
                        }))
                    }),
                ),
            )
            .child(self.result("service", p))
    }

    pub(super) fn endpoint_settings(
        &mut self,
        prefs: &Settings,
        narrow: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = Palette::get(cx);
        let api = prefs.active().cloned().unwrap_or_default();
        let options: Vec<(String, String)> = prefs
            .clash_apis
            .iter()
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();
        let connected = self.traffic_status == "已连接";
        let connecting = self.traffic_status == "连接中";
        let host_label = host_for_display(&self.endpoint);
        let (dot_color, halo_color, status_text, is_err) = if connected {
            (
                p.success_dot(),
                p.success_soft(),
                format!("已连接 {host_label} · 共 {} 个端点", prefs.clash_apis.len()),
                false,
            )
        } else if connecting {
            (
                p.warning_dot(),
                p.warning_soft(),
                format!(
                    "正在连接 {host_label} · 共 {} 个端点",
                    prefs.clash_apis.len()
                ),
                false,
            )
        } else {
            (
                p.danger_fill(),
                p.danger_soft(),
                format!("无法连接 {host_label}"),
                true,
            )
        };

        let edit_open = self.settings_layout.api_edit == Some(false);
        let add_open = self.settings_layout.api_edit == Some(true);
        let controls = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .when(!narrow, |d| d.w(px(340.)).flex_none())
            .child(
                div()
                    .size(px(14.))
                    .mx(px(4.))
                    .flex_none()
                    .rounded_full()
                    .bg(halo_color)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().size(px(8.)).rounded_full().bg(dot_color)),
            )
            .child(div().flex_1().min_w_0().child(picker(
                "saved-endpoints",
                api.id.clone(),
                options,
                |v, id, cx| {
                    v.run_setting("api", "switch_api", Values::from([("id".into(), id)]), cx)
                },
                cx,
            )))
            .child(
                icon_btn("edit-api-btn", "edit", edit_open, false, p).on_click(cx.listener(
                    |v, _, _, cx| {
                        if v.settings_layout.api_edit == Some(false) {
                            v.settings_layout.api_edit = None;
                        } else {
                            v.settings_layout.api_edit = Some(false);
                            v.settings_layout.protocol = None;
                            for k in ["name", "host", "port", "secret"] {
                                v.settings_layout.fields.remove(k);
                            }
                        }
                        cx.notify();
                    },
                )),
            )
            .child(
                icon_btn("add-api-btn", "plus", add_open, false, p).on_click(cx.listener(
                    |v, _, _, cx| {
                        if v.settings_layout.api_edit == Some(true) {
                            v.settings_layout.api_edit = None;
                        } else {
                            v.settings_layout.api_edit = Some(true);
                            v.settings_layout.protocol = None;
                            for k in ["name", "host", "port", "secret"] {
                                v.settings_layout.fields.remove(k);
                            }
                        }
                        cx.notify();
                    },
                )),
            );

        let top_row = div()
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
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(crate::theme::text_px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("当前端点"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if is_err { p.danger() } else { p.muted() })
                            .truncate()
                            .child(status_text),
                    ),
            )
            .child(controls);

        let mut form = div().child(top_row);
        if let Some(new) = self.settings_layout.api_edit {
            let parsed = if new {
                None
            } else {
                reqwest::Url::parse(&api.url).ok()
            };
            let port = parsed
                .as_ref()
                .and_then(|u| u.port())
                .map(|p| p.to_string())
                .unwrap_or_default();
            let scheme = match parsed.as_ref().map(|u| u.scheme()) {
                Some("https" | "wss") => "https",
                _ => "http",
            };
            let protocol = self
                .settings_layout
                .protocol
                .clone()
                .unwrap_or_else(|| scheme.to_owned());
            let host = parsed
                .map(|mut u| {
                    let _ = u.set_port(None);
                    u.as_str()
                        .split_once("://")
                        .map(|(_, v)| v.trim_end_matches('/').to_string())
                        .unwrap_or_default()
                })
                .unwrap_or_else(|| if new { String::new() } else { api.url.clone() });
            let field_wrap = |label: &'static str| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_xs().text_color(p.muted()).child(label))
            };
            let name = self.field(
                "name",
                if new { "" } else { &api.name },
                if new { "端点 2" } else { "默认端点" },
                w,
                cx,
            );
            let host_input = self.field("host", &host, "127.0.0.1", w, cx);
            let port_input = self.field("port", &port, "9090", w, cx);
            let secret = self.field(
                "secret",
                if new { "" } else { &api.secret },
                "留空表示无密钥",
                w,
                cx,
            );
            let protocol_control = picker(
                "api-protocol",
                protocol.clone(),
                ["http", "https"].map(|v| (v.into(), v.into())).to_vec(),
                |v, s, cx| {
                    v.settings_layout.protocol = Some(s);
                    cx.notify();
                },
                cx,
            );

            let can_delete = !new && prefs.clash_apis.len() > 1;
            let delete_id = api.id.clone();
            let delete_name = api.name.clone();
            let next_name = prefs
                .clash_apis
                .iter()
                .find(|a| a.id != api.id)
                .map(|a| a.name.clone())
                .unwrap_or_default();
            let save_id = api.id.clone();
            let save_proto = protocol.clone();

            let actions = div()
                .flex()
                .items_center()
                .when(!new, |d| d.justify_between())
                .when(new, |d| d.justify_end())
                .gap_2()
                .when(!new, |d| {
                    d.child(
                        danger_btn("delete-endpoint", "删除端点", true, !can_delete, p).when(
                            can_delete,
                            |b| {
                                b.on_click(cx.listener(move |v, _, _, cx| {
                                    v.settings_layout.dialog =
                                        Some(SettingsDialog::ConfirmDeleteEndpoint {
                                            id: delete_id.clone(),
                                            name: delete_name.clone(),
                                            next_name: next_name.clone(),
                                        });
                                    cx.notify();
                                }))
                            },
                        ),
                    )
                })
                .child(
                    div().flex().items_center().gap(px(6.)).child(
                        action_btn(
                            "save-endpoint",
                            if new {
                                "新增并切换"
                            } else {
                                "保存端点"
                            },
                            false,
                            p,
                        )
                        .on_click(cx.listener(move |v, _, _, cx| {
                            let mut values = v.values(&["name", "host", "port", "secret"], cx);
                            values.insert("protocol".into(), save_proto.clone());
                            values.insert("id".into(), save_id.clone());
                            v.run_setting(
                                "api",
                                if new { "add_api" } else { "save_api" },
                                values,
                                cx,
                            );
                        })),
                    ),
                );

            let fields = div()
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(crate::theme::text_px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(if new { "新增端点" } else { "编辑端点" }),
                        )
                        .child(
                            div().text_xs().text_color(p.muted()).child(if new {
                                "新增后会自动切换到这个端点。主机里可以直接粘贴完整地址，如 http://127.0.0.1:9090。"
                            } else {
                                "保存后面板会立即重新连接。主机里可以直接粘贴完整地址，如 http://127.0.0.1:9090。"
                            }),
                        ),
                )
                .child(field_wrap("名称").child(glass_input("api-name-input", &name, false, p)))
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .when(narrow, |s| s.flex_col())
                        .child(
                            field_wrap("协议")
                                .when(!narrow, |s| s.w(px(96.)).flex_none())
                                .child(protocol_control),
                        )
                        .child(
                            field_wrap("主机")
                                .when(!narrow, |s| s.flex_1().min_w_0())
                                .child(glass_input("api-host-input", &host_input, true, p)),
                        )
                        .child(
                            field_wrap("端口")
                                .when(!narrow, |s| s.w(px(96.)).flex_none())
                                .child(glass_input("api-port-input", &port_input, true, p)),
                        ),
                )
                .child(field_wrap("访问密钥").child(glass_input("api-secret-input", &secret, true, p)))
                .child(actions);

            form = form.child(divider(p)).child(fields);
        }
        form.child(self.result("api", p))
    }

    fn save_network(
        &mut self,
        ipv6: bool,
        group: Option<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        let mut values = self.values(&["test_url"], cx);
        values.insert("ipv6".into(), if ipv6 { "on" } else { "off" }.into());
        if let Some((name, url)) = group {
            values.insert("test_url".into(), self.latency_test_url.clone());
            values.insert("group".into(), name);
            values.insert("group_url".into(), url);
        }
        self.run_setting("network", "test_settings", values, cx);
    }

    pub(super) fn network_settings(
        &mut self,
        prefs: &Settings,
        narrow: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = Palette::get(cx);
        let subscribe = !self.settings_layout.fields.contains_key("test_url");
        let input = self.field(
            "test_url",
            &prefs.latency_test_url,
            "https://www.gstatic.com/generate_204",
            w,
            cx,
        );
        if subscribe {
            cx.subscribe(&input, |this, field, event: &gpui_component::input::InputEvent, cx| {
                if matches!(event, gpui_component::input::InputEvent::Blur | gpui_component::input::InputEvent::PressEnter { .. })
                    && !this.settings_layout.pending.contains("network")
                    && field.read(cx).value().as_str() != this.latency_test_url
                {
                    this.save_network(this.ipv6, None, cx);
                }
            }).detach();
        }
        let ipv6 = prefs.ipv6_test_enabled;
        let default_url_row =
            div()
                .px_4()
                .py_3()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(crate::theme::text_px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .child("默认测速地址"),
                        )
                        .child(div().text_xs().text_color(p.muted()).child(
                            "没有单独指定测试地址的代理组会使用这里的 URL。留空则恢复默认地址。",
                        )),
                )
                .child(
                    glass_input("network-default-url-field", &input, true, p)
                        .on_key_down(cx.listener(move |this, key: &KeyDownEvent, window, cx| {
                            if key.keystroke.key == "escape" {
                                input.update(cx, |field, cx| field.set_value(this.latency_test_url.clone(), window, cx));
                                cx.stop_propagation();
                            }
                        })),
                );

        let group_summary = if prefs.group_test_urls.is_empty() {
            "暂未配置".into()
        } else {
            format!("已配置 {} 个代理组", prefs.group_test_urls.len())
        };
        let mut card = div()
            .child(default_url_row)
            .child(divider(p))
            .child(self.disclosure("groups", "代理组专用地址", group_summary, cx));

        if self.settings_layout.open.contains("groups") {
            let name = self.field("group", "", "代理组名称", w, cx);
            let url = self.field("group_url", "", "测速地址", w, cx);
            let mut mappings = div().px_4().pb_3().flex().flex_col().gap(px(6.));
            let mut entries: Vec<_> = prefs.group_test_urls.iter().collect();
            entries.sort_by_key(|(name, _)| *name);
            for (idx, (group, source)) in entries.into_iter().enumerate() {
                let (group, source) = (group.clone(), source.clone());
                let n = name.clone();
                let u = url.clone();
                let remove = group.clone();
                mappings = mappings.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .w(px(140.))
                                .flex_none()
                                .px(px(10.))
                                .py(px(4.))
                                .rounded_full()
                                .bg(p.primary_soft())
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.accent())
                                .truncate()
                                .child(crate::emoji::text(group.clone())),
                        )
                        .when(!narrow, |d| {
                            d.child(div().text_xs().text_color(p.muted()).child("→"))
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_family("Consolas")
                                .text_xs()
                                .text_color(p.muted())
                                .truncate()
                                .child(source.clone()),
                        )
                        .child(
                            icon_btn(("edit-map", idx), "edit", false, false, p).on_click(
                                cx.listener(move |_, _, w, cx| {
                                    n.update(cx, |i, cx| i.set_value(group.clone(), w, cx));
                                    u.update(cx, |i, cx| i.set_value(source.clone(), w, cx));
                                }),
                            ),
                        )
                        .child(
                            icon_btn(("remove-map", idx), "trash", false, true, p).on_click(
                                cx.listener(move |v, _, _, cx| {
                                    let mut previous = None;
                                    let result = crate::service::data_dir().and_then(|dir| {
                                        Settings::edit(&dir, |prefs| {
                                            previous = prefs.group_test_urls.remove(&remove);
                                            Ok(())
                                        })
                                    });
                                    if let Err(error) = result {
                                        v.settings_layout.results.insert("network", error);
                                    } else if let Some(url) = previous {
                                        let started = std::time::Instant::now();
                                        v.settings_layout.mapping_undo =
                                            Some((started, remove.clone(), url));
                                        v.expire_settings_undo(started, cx);
                                    }
                                    cx.notify();
                                }),
                            ),
                        ),
                );
            }
            if let Some((_, group, url)) = &self.settings_layout.mapping_undo {
                let group = group.clone();
                let url = url.clone();
                mappings = mappings.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted())
                                .child(format!("已移除「{group}」的测速地址")),
                        )
                        .child(
                            action_btn("undo-map", "撤销", false, p).on_click(cx.listener(
                                move |v, _, _, cx| {
                                    let result = crate::service::data_dir().and_then(|dir| {
                                        Settings::edit(&dir, |prefs| {
                                            prefs
                                                .group_test_urls
                                                .entry(group.clone())
                                                .or_insert_with(|| url.clone());
                                            Ok(())
                                        })
                                    });
                                    match result {
                                        Ok(()) => v.settings_layout.mapping_undo = None,
                                        Err(error) => {
                                            v.settings_layout.results.insert("network", error);
                                        }
                                    }
                                    cx.notify();
                                },
                            )),
                        ),
                );
            }
            let add_row = div()
                .pt_1()
                .flex()
                .when(narrow, |d| d.flex_col().gap_2())
                .when(!narrow, |d| d.items_center().gap(px(6.)))
                .child(
                    div()
                        .when(!narrow, |d| d.w(px(140.)).flex_none())
                        .child(glass_input("map-group-input", &name, false, p)),
                )
                .when(!narrow, |d| {
                    d.child(div().text_xs().text_color(p.muted()).child("→"))
                })
                .child(div().flex_1().min_w_0().child(glass_input("map-url-input", &url, true, p)))
                .child(
                    action_btn("add-map", "添加映射", false, p).on_click(cx.listener(
                        move |v, _, _, cx| {
                            let name_v = name.read(cx).value().to_string();
                            let url_v = url.read(cx).value().to_string();
                            if name_v.trim().is_empty() || url_v.trim().is_empty() {
                                v.settings_layout
                                    .results
                                    .insert("network", "请填写代理组名称和测速地址".into());
                                cx.notify();
                            } else {
                                v.save_network(ipv6, Some((name_v, url_v)), cx);
                            }
                        },
                    )),
                );
            card = card.child(mappings.child(add_row));
        }

        card.child(divider(p))
            .child(setting_row(
                "IPv6 连通性测试",
                "测速时额外检查节点的 IPv6 可用性。",
                pill_switch("set-ipv6", ipv6, p)
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.edit_preferences(
                            |p| {
                                p.ipv6_test_enabled = !ipv6;
                                Ok(())
                            },
                            cx,
                        )
                    }))
                    .into_any_element(),
                false,
                p,
            ))
            .child(self.result("network", p))
    }

    pub(super) fn dns_settings(
        &mut self,
        prefs: &Settings,
        width: f32,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = Palette::get(cx);
        let domain = self.field("domain", "", "www.google.com", w, cx);
        let record = if self.settings_layout.dns_record.is_empty() {
            "A"
        } else {
            &self.settings_layout.dns_record
        }
        .to_owned();
        let resolver = if self.settings_layout.dns_server.is_empty() {
            "system"
        } else {
            &self.settings_layout.dns_server
        }
        .to_owned();
        let records = ["A", "AAAA", "CNAME"]
            .map(|s| (s.into(), s.into()))
            .to_vec();
        let servers = [
            ("system", "本机 DNS"),
            ("8.8.8.8", "Google"),
            ("1.1.1.1", "Cloudflare"),
            ("223.5.5.5", "AliDNS"),
            ("119.29.29.29", "DNSPod"),
            ("custom", "自定义"),
        ]
        .map(|(v, l)| (v.into(), l.into()))
        .to_vec();
        let wide = width >= 620.;
        let label = |s: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(div().text_xs().text_color(p.muted()).child(s))
        };
        let mut controls = div()
            .px_4()
            .pt_4()
            .pb(px(14.))
            .flex()
            .gap(px(10.))
            .when(!wide, |s| s.flex_col())
            .when(wide, |s| s.items_end())
            .child(
                label("域名").when(wide, |s| s.flex_1().min_w(px(150.))).child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(div().flex_1().min_w_0().child(glass_input("dns-domain-input", &domain, true, p)))
                        .when(!prefs.dns_history.is_empty(), |d| {
                            d.child(
                                icon_btn("open-dns-history", "chevron-down", false, false, p)
                                    .on_click(cx.listener(|v, _, _, cx| {
                                        v.settings_layout.dns_history_open =
                                            !v.settings_layout.dns_history_open;
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
            )
            .child(
                label("记录")
                    .when(wide, |s| s.w(px(80.)).flex_none())
                    .child(picker(
                        "record-type",
                        record.clone(),
                        records,
                        |v, s, cx| {
                            v.settings_layout.dns_record = s;
                            cx.notify();
                        },
                        cx,
                    )),
            )
            .child(
                label("解析器")
                    .when(wide, |s| s.w(px(210.)).flex_none())
                    .child(picker(
                        "dns-server",
                        resolver.clone(),
                        servers,
                        |v, s, cx| {
                            v.settings_layout.dns_server = s;
                            cx.notify();
                        },
                        cx,
                    )),
            );
        if resolver == "custom" {
            let server = self.field("server", "", "IP 或 IP:端口", w, cx);
            controls = controls.child(
                label("自定义")
                    .when(wide, |s| s.w(px(130.)).flex_none())
                    .child(glass_input("dns-custom-input", &server, true, p)),
            );
        }
        let busy = self.settings_layout.pending.contains("dns");
        let rec_submit = record.clone();
        let res_submit = resolver.clone();
        controls = controls.child(
            action_btn("lookup", if busy { "查询中…" } else { "查询" }, busy, p).when(!busy, |b| {
                b.on_click(cx.listener(move |v, _, _, cx| {
                    let vals = v.values(&["domain", "server"], cx);
                    v.run_dns_query(
                        vals.get("domain").cloned().unwrap_or_default(),
                        rec_submit.clone(),
                        res_submit.clone(),
                        vals.get("server").cloned().unwrap_or_default(),
                        true,
                        cx,
                    );
                }))
            }),
        );

        let mut root = div().child(controls);
        if self.settings_layout.dns_history_open {
            for (i, saved) in prefs.dns_history.iter().enumerate() {
                let value = saved.clone();
                let remove = saved.clone();
                let input = domain.clone();
                root = root.child(
                    div()
                        .mx_4()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .id(("dns-history", i))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .cursor_pointer()
                                .px_2()
                                .py_1()
                                .font_family("Consolas")
                                .text_xs()
                                .child(saved.clone())
                                .on_click(cx.listener(move |v, _, w, cx| {
                                    input.update(cx, |field, cx| {
                                        field.set_value(value.clone(), w, cx)
                                    });
                                    v.settings_layout.dns_history_open = false;
                                    cx.notify();
                                })),
                        )
                        .child(
                            icon_btn(("dns-delete", i), "trash", false, true, p).on_click(
                                cx.listener(move |v, _, _, cx| {
                                    if let Err(error) = crate::service::data_dir()
                                        .and_then(|dir| Settings::dns_history(&dir, &remove, false))
                                    {
                                        v.settings_layout.results.insert("dns", error);
                                    }
                                    cx.notify();
                                }),
                            ),
                        ),
                );
            }
        }
        root = root.child(self.result("dns", p));

        if let Some(view) = &self.settings_layout.dns_view {
            let mut box_el = div()
                .mx_4()
                .mb(px(14.))
                .rounded(px(14.))
                .overflow_hidden()
                .bg(p.fill())
                .flex()
                .flex_col();
            if view.records.is_empty() {
                box_el = box_el.child(
                    div()
                        .px_3()
                        .py_6()
                        .flex()
                        .justify_center()
                        .text_xs()
                        .text_color(p.muted())
                        .child("无记录"),
                );
            } else {
                for (i, rec) in view.records.iter().enumerate() {
                    if i > 0 {
                        box_el = box_el.child(div().h(px(0.5)).w_full().bg(p.line()));
                    }
                    box_el = box_el.child(
                        div()
                            .px_3()
                            .py(px(10.))
                            .flex()
                            .items_start()
                            .gap_3()
                            .child(
                                div()
                                    .flex_none()
                                    .mt(px(2.))
                                    .px_2()
                                    .py(px(1.))
                                    .rounded_full()
                                    .bg(p.fill_strong())
                                    .text_size(crate::theme::text_px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.ink())
                                    .child(rec.record_type.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .font_family("Consolas")
                                            .text_xs()
                                            .truncate()
                                            .child(rec.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::theme::text_px(11.))
                                            .text_color(p.muted())
                                            .child(format!("TTL {}", rec.ttl)),
                                    )
                                    .when(!rec.geo.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .text_size(crate::theme::text_px(11.))
                                                .text_color(p.muted())
                                                .truncate()
                                                .child(rec.geo.clone()),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .font_family("Consolas")
                                    .text_xs()
                                    .text_color(p.ink())
                                    .child(rec.value.clone()),
                            ),
                    );
                }
            }
            box_el = box_el.child(div().h(px(0.5)).w_full().bg(p.line())).child(
                div()
                    .px_3()
                    .py_2()
                    .text_size(crate::theme::text_px(11.))
                    .text_color(p.muted())
                    .child(view.source_label.clone()),
            );
            root = root.child(box_el);
        }

        root
    }

    pub(super) fn update_settings(
        &mut self,
        prefs: &Settings,
        narrow: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Div, Div) {
        let p = Palette::get(cx);
        let row = |title, desc: String, ctl| setting_row(title, desc, ctl, narrow, p);
        let active_source = if prefs.core_update_source.is_empty() {
            "official"
        } else {
            &prefs.core_update_source
        };
        let active_channel = if prefs.core_update_channel.is_empty() {
            "stable"
        } else {
            &prefs.core_update_channel
        };
        let mut values = Values::from([
            ("source".into(), active_source.into()),
            ("channel".into(), active_channel.into()),
            ("repo".into(), prefs.core_update_custom_repo.clone()),
            ("mirror".into(), prefs.core_update_mirror.clone()),
        ]);
        let settings = values.clone();
        values.extend([
            ("core".into(), prefs.singbox_path.clone()),
            ("token".into(), prefs.github_token.clone()),
            (
                "installed".into(),
                self.summary["version"].as_str().unwrap_or("").to_owned(),
            ),
        ]);
        let core_busy = self.settings_layout.pending.contains("core-update");
        let core_status = if core_busy {
            "正在检查上游版本…".to_owned()
        } else if let Some(msg) = self.settings_layout.results.get("core-update") {
            msg.clone()
        } else {
            "尚未检查上游版本".to_owned()
        };
        let check = action_btn(
            "core-check",
            if core_busy {
                "检查中"
            } else {
                "检查核心更新"
            },
            core_busy,
            p,
        )
        .when(!core_busy, |b| {
            b.on_click(cx.listener(move |v, _, _, cx| {
                v.run_setting("core-update", "core_check", values.clone(), cx)
            }))
        });

        let mut core = div().child(row("sing-box 核心", core_status, check.into_any_element()));
        for (key, title, active, options) in [
            (
                "source",
                "更新源",
                active_source,
                vec![
                    ("official", "官方核心 (SagerNet/sing-box)"),
                    ("ref1nd", "reF1nd 核心"),
                    ("custom", "自定义仓库"),
                ],
            ),
            (
                "channel",
                "版本通道",
                active_channel,
                vec![("stable", "稳定版"), ("testing", "测试版")],
            ),
        ] {
            let current = settings.clone();
            core = core.child(divider(p)).child(row(
                title,
                String::new(),
                div()
                    .when(!narrow, |d| d.w(px(240.)))
                    .child(picker(
                        key,
                        active.into(),
                        options
                            .into_iter()
                            .map(|(k, v)| (k.into(), v.into()))
                            .collect(),
                        move |v, value, cx| {
                            let mut next = current.clone();
                            next.insert(key.into(), value);
                            v.run_setting("core-update", "update_preferences", next, cx);
                        },
                        cx,
                    ))
                    .into_any_element(),
            ));
            if key == "source" && active == "custom" {
                core = core.child(divider(p)).child(self.stacked_update_field(
                    "repo",
                    "GitHub 仓库",
                    "写成 owner/repo，从它的 Releases 下载 Windows 版核心。",
                    "SagerNet/sing-box",
                    &settings,
                    w,
                    cx,
                ));
            }
        }
        core = core.child(divider(p)).child(self.stacked_update_field(
            "mirror",
            "下载镜像前缀",
            "可选。下载核心和面板更新时都会把它加在 GitHub 地址前面；留空直连 GitHub。镜像能看到并改写下载内容，只填你信任的地址。",
            "https://mirror.example.com/",
            &settings,
            w,
            cx,
        ));

        let auto = prefs.panel_auto_check_update.unwrap_or(true);
        let panel_busy = self.settings_layout.pending.contains("panel-update");
        let panel_status = if panel_busy {
            "正在检查上游版本…".to_owned()
        } else if let Some(msg) = self.settings_layout.results.get("panel-update") {
            msg.clone()
        } else {
            "尚未检查上游版本".to_owned()
        };
        let has_token = !prefs.github_token.trim().is_empty();
        let current_token = prefs.github_token.clone();

        let panel_top = div()
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
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("singboard 面板"),
                            )
                            .child(
                                div()
                                    .font_family("Consolas")
                                    .text_xs()
                                    .text_color(p.muted())
                                    .child(singboard_stream::update::CURRENT),
                            ),
                    )
                    .child(div().text_xs().text_color(p.muted()).child(panel_status)),
            )
            .child(
                action_btn(
                    "panel-check",
                    if panel_busy {
                        "检查中"
                    } else {
                        "检查面板更新"
                    },
                    panel_busy,
                    p,
                )
                .when(!panel_busy, |b| {
                    b.on_click(cx.listener(|v, _, _, cx| v.check_panel_update(cx)))
                }),
            );

        let token_row = div()
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
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("GitHub Token"),
                            )
                            .when(has_token, |el| {
                                el.child(
                                    div()
                                        .px_2()
                                        .py(px(1.))
                                        .rounded_full()
                                        .bg(p.fill())
                                        .text_size(crate::theme::text_px(11.))
                                        .text_color(p.muted())
                                        .child("已填写"),
                                )
                            }),
                    )
                    .child(div().text_xs().text_color(p.muted()).child(if has_token {
                        "检查核心和面板更新时带上 Token，面板下载优先走 GitHub API。"
                    } else {
                        "可选。填写后检查核心和面板更新不受匿名限流影响。"
                    })),
            )
            .child(
                action_btn(
                    "open-token-dialog",
                    if has_token { "修改" } else { "填写" },
                    false,
                    p,
                )
                .on_click(cx.listener(move |v, _, window, cx| {
                    let tok = current_token.clone();
                    let input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(tok)
                            .placeholder("github_pat_… 或 ghp_…")
                            .masked(true)
                    });
                    v.settings_layout.dialog = Some(SettingsDialog::GithubToken {
                        input,
                        reveal: false,
                        error: String::new(),
                    });
                    cx.notify();
                })),
            );

        let panel = div()
            .child(panel_top)
            .child(divider(p))
            .child(setting_row(
                "启动时自动检查",
                "打开面板后静默检查是否有新版本。",
                pill_switch("auto-check", auto, p)
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.edit_preferences(
                            |p| {
                                p.panel_auto_check_update = Some(!auto);
                                Ok(())
                            },
                            cx,
                        )
                    }))
                    .into_any_element(),
                false,
                p,
            ))
            .child(divider(p))
            .child(token_row);

        (core, panel)
    }

    fn stacked_update_field(
        &mut self,
        key: &'static str,
        label: &'static str,
        desc: &'static str,
        placeholder: &'static str,
        values: &Values,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = Palette::get(cx);
        let subscribe = !self.settings_layout.fields.contains_key(key);
        let saved = values[key].clone();
        let input = self.field(key, &saved, placeholder, w, cx);
        if subscribe {
            cx.subscribe(&input, move |this, field, event: &gpui_component::input::InputEvent, cx| {
                if matches!(
                    event,
                    gpui_component::input::InputEvent::Blur
                        | gpui_component::input::InputEvent::PressEnter { .. }
                ) && !this.settings_layout.pending.contains("core-update")
                {
                    let prefs = crate::maintenance::preferences().unwrap_or_default();
                    let current_val = if key == "repo" {
                        prefs.core_update_custom_repo.as_str()
                    } else {
                        prefs.core_update_mirror.as_str()
                    };
                    let next_val = field.read(cx).value().to_string();
                    if next_val != current_val {
                        let active_source = if prefs.core_update_source.is_empty() {
                            "official"
                        } else {
                            &prefs.core_update_source
                        };
                        let active_channel = if prefs.core_update_channel.is_empty() {
                            "stable"
                        } else {
                            &prefs.core_update_channel
                        };
                        let mut next = Values::from([
                            ("source".into(), active_source.into()),
                            ("channel".into(), active_channel.into()),
                            ("repo".into(), prefs.core_update_custom_repo.clone()),
                            ("mirror".into(), prefs.core_update_mirror.clone()),
                        ]);
                        next.insert(key.into(), next_val);
                        this.run_setting("core-update", "update_preferences", next, cx);
                    }
                }
            })
            .detach();
        }
        div()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(crate::theme::text_px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .line_height(crate::theme::text_px(17.))
                            .text_color(p.muted())
                            .child(desc),
                    ),
            )
            .child(
                glass_input(SharedString::from(format!("update-field-{key}")), &input, true, p).on_key_down(cx.listener(
                    move |_, key_ev: &KeyDownEvent, window, cx| {
                        if key_ev.keystroke.key == "escape" {
                            input.update(cx, |field, cx| field.set_value(saved.clone(), window, cx));
                            cx.stop_propagation();
                        }
                    },
                )),
            )
    }

    pub(crate) fn render_settings_dialog(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let p = Palette::get(cx);
        let (dialog, leaving) = match (&self.settings_layout.dialog, &self.leaving_dialog) {
            (Some(dialog), _) => (dialog, false),
            (None, Some((_, dialog::LeavingDialog::Settings(dialog)))) => (dialog, true),
            _ => return None,
        };
        let body: Div = match dialog {
            SettingsDialog::ConfirmStop => div()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .text_size(crate::theme::text_px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("停止核心？"),
                )
                .child(
                    div()
                        .text_sm()
                        .line_height(crate::theme::text_px(20.))
                        .text_color(p.muted())
                        .child("代理会中断，直到再次启动核心。"),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            div()
                                .id("dlg-stop-cancel")
                                .cursor_pointer()
                                .px_3()
                                .py(px(6.))
                                .rounded_lg()
                                .hover(|s| s.bg(p.hover()))
                                .text_sm()
                                .child("取消")
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.close_settings_dialog(cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("dlg-stop-confirm")
                                .cursor_pointer()
                                .px_4()
                                .py(px(6.))
                                .rounded_lg()
                                .bg(p.danger_fill())
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(0xffffff))
                                .child("停止核心")
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.close_settings_dialog(cx);
                                    v.tray_service("stop", cx);
                                    cx.notify();
                                })),
                        ),
                ),
            SettingsDialog::ConfirmUninstall { running } => div()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .text_size(crate::theme::text_px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("卸载 Windows 服务？"),
                )
                .child(
                    div()
                        .text_sm()
                        .line_height(crate::theme::text_px(20.))
                        .text_color(p.muted())
                        .child(if *running {
                            "核心将停止，代理会中断。之后要在后台运行核心，需要重新安装服务。"
                        } else {
                            "之后要在后台运行核心，需要重新安装服务。"
                        }),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            div()
                                .id("dlg-uninst-cancel")
                                .cursor_pointer()
                                .px_3()
                                .py(px(6.))
                                .rounded_lg()
                                .hover(|s| s.bg(p.hover()))
                                .text_sm()
                                .child("取消")
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.close_settings_dialog(cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("dlg-uninst-confirm")
                                .cursor_pointer()
                                .px_4()
                                .py(px(6.))
                                .rounded_lg()
                                .bg(p.danger_fill())
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(0xffffff))
                                .child("卸载服务")
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.close_settings_dialog(cx);
                                    v.run_setting(
                                        "service-op",
                                        "uninstall_service",
                                        Values::new(),
                                        cx,
                                    );
                                })),
                        ),
                ),
            SettingsDialog::ConfirmDeleteEndpoint {
                id,
                name,
                next_name,
            } => {
                let del_id = id.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .text_size(crate::theme::text_px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("删除端点「{name}」？")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .line_height(crate::theme::text_px(20.))
                            .text_color(p.muted())
                            .child(format!("面板会改为连接「{next_name}」。")),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("dlg-del-api-cancel")
                                    .cursor_pointer()
                                    .px_3()
                                    .py(px(6.))
                                    .rounded_lg()
                                    .hover(|s| s.bg(p.hover()))
                                    .text_sm()
                                    .child("取消")
                                    .on_click(cx.listener(|v, _, _, cx| {
                                        v.close_settings_dialog(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("dlg-del-api-confirm")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(6.))
                                    .rounded_lg()
                                    .bg(p.danger_fill())
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(0xffffff))
                                    .child("删除端点")
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        v.close_settings_dialog(cx);
                                        v.run_setting(
                                            "api",
                                            "delete_api",
                                            Values::from([("id".into(), del_id.clone())]),
                                            cx,
                                        );
                                    })),
                            ),
                    )
            }
            SettingsDialog::GithubToken {
                input,
                reveal,
                error,
            } => {
                let has_existing = !crate::maintenance::preferences()
                    .map(|pr| pr.github_token)
                    .unwrap_or_default()
                    .trim()
                    .is_empty();
                let input_el = input.clone();
                let next_reveal = !*reveal;
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .text_size(crate::theme::text_px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("GitHub Token"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .line_height(crate::theme::text_px(20.))
                            .text_color(p.muted())
                            .child("填写后，检查核心和面板更新都会带上这个 Token，不受匿名限流影响；下载面板更新也会优先通过 GitHub API，失败时再改走 Release 直链。Token 只保存在本机，只发给 api.github.com。"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().text_xs().text_color(p.muted()).child("Token"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().flex_1().min_w_0().child(glass_input("dlg-token-input", input, true, p)))
                                    .child(
                                        action_btn(
                                            "dlg-token-reveal",
                                            if *reveal { "隐藏" } else { "显示" },
                                            false,
                                            p,
                                        )
                                        .on_click(cx.listener(move |v, _, window, cx| {
                                            if let Some(SettingsDialog::GithubToken {
                                                input,
                                                reveal,
                                                ..
                                            }) = &mut v.settings_layout.dialog
                                            {
                                                *reveal = next_reveal;
                                                input.update(cx, |s, cx| {
                                                    s.set_masked(!next_reveal, window, cx)
                                                });
                                            }
                                            cx.notify();
                                        })),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(crate::theme::text_px(11.))
                                    .text_color(p.muted())
                                    .child("公开仓库不需要任何权限，建议创建一个不勾选权限的 Fine-grained Token。"),
                            ),
                    )
                    .when(!error.is_empty(), |el| {
                        el.child(div().text_xs().text_color(p.danger()).child(error.clone()))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .when(has_existing, |el| {
                                el.child(
                                    div().mr_auto().child(
                                        danger_btn("dlg-token-clear", "清除 Token", false, false, p)
                                            .on_click(cx.listener(|v, _, _, cx| {
                                                v.close_settings_dialog(cx);
                                                v.run_setting(
                                                    "panel-update",
                                                    "save_token",
                                                    Values::from([("token".into(), String::new())]),
                                                    cx,
                                                );
                                            })),
                                    ),
                                )
                            })
                            .child(
                                div()
                                    .id("dlg-token-cancel")
                                    .cursor_pointer()
                                    .px_3()
                                    .py(px(6.))
                                    .rounded_lg()
                                    .hover(|s| s.bg(p.hover()))
                                    .text_sm()
                                    .child("取消")
                                    .on_click(cx.listener(|v, _, _, cx| {
                                        v.close_settings_dialog(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(route_btn("dlg-token-save", "保存", false, p).on_click(
                                cx.listener(move |v, _, _, cx| {
                                    let val = input_el.read(cx).value().trim().to_owned();
                                    if let Some(err) = validate_github_token(&val) {
                                        if let Some(SettingsDialog::GithubToken { error, .. }) =
                                            &mut v.settings_layout.dialog
                                        {
                                            *error = err.into();
                                        }
                                        cx.notify();
                                        return;
                                    }
                                    v.close_settings_dialog(cx);
                                    v.run_setting(
                                        "panel-update",
                                        "save_token",
                                        Values::from([("token".into(), val)]),
                                        cx,
                                    );
                                }),
                            )),
                    )
            }
            SettingsDialog::ConfirmCoreUpdate {
                summary,
                target,
                ticket,
                values,
                busy,
                error,
            } => {
                let ticket_val = ticket.clone();
                let values_val = values.clone();
                let is_busy = *busy;
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .text_size(crate::theme::text_px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("发现新核心版本"),
                    )
                    .child(
                        div()
                            .rounded_lg()
                            .bg(p.fill())
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_xs()
                            .child(
                                div()
                                    .font_family("Consolas")
                                    .text_color(p.ink())
                                    .child(summary.clone()),
                            )
                            .when(!target.is_empty(), |el| {
                                el.child(
                                    div()
                                        .font_family("Consolas")
                                        .text_color(p.muted())
                                        .child(format!("目标：{target}")),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_sm()
                            .line_height(crate::theme::text_px(20.))
                            .text_color(p.muted())
                            .child(if is_busy {
                                crate::core_actions::progress()
                            } else {
                                "更新会先停止核心，替换后自动重启，期间代理短暂中断。".into()
                            }),
                    )
                    .when(!error.is_empty(), |el| {
                        el.child(div().text_xs().text_color(p.danger()).child(error.clone()))
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .when(!is_busy, |el| {
                                el.child(
                                    div()
                                        .id("dlg-core-cancel")
                                        .cursor_pointer()
                                        .px_3()
                                        .py(px(6.))
                                        .rounded_lg()
                                        .hover(|s| s.bg(p.hover()))
                                        .text_sm()
                                        .child("稍后")
                                        .on_click(cx.listener(|v, _, _, cx| {
                                            v.close_settings_dialog(cx);
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(
                                route_btn(
                                    "dlg-core-confirm",
                                    if is_busy {
                                        "更新中…"
                                    } else {
                                        "立即更新"
                                    },
                                    is_busy,
                                    p,
                                )
                                .when(!is_busy, |b| {
                                    b.on_click(cx.listener(move |v, _, _, cx| {
                                        if let Some(SettingsDialog::ConfirmCoreUpdate {
                                            busy,
                                            error,
                                            ..
                                        }) = &mut v.settings_layout.dialog
                                        {
                                            *busy = true;
                                            error.clear();
                                        }
                                        let t = ticket_val.clone();
                                        let vals = values_val.clone();
                                        let job = singboard_stream::runtime().spawn(async move {
                                            let dir = crate::service::data_dir()?;
                                            crate::core_actions::run(
                                                &dir,
                                                "core_prepare",
                                                &vals,
                                                Some(&t),
                                            )
                                            .await
                                        });
                                        cx.spawn(async move |weak, cx| {
                                            let r = job
                                                .await
                                                .map_err(|e| e.to_string())
                                                .and_then(|r| r);
                                            let _ = weak.update(cx, |v, cx| {
                                                match r {
                                                    Ok(reply) => {
                                                        v.close_settings_dialog(cx);
                                                        v.settings_layout
                                                            .results
                                                            .insert("core-update", reply.text);
                                                        v.refresh_local_state(cx);
                                                    }
                                                    Err(e) => {
                                                        if let Some(
                                                            SettingsDialog::ConfirmCoreUpdate {
                                                                busy,
                                                                error,
                                                                ..
                                                            },
                                                        ) = &mut v.settings_layout.dialog
                                                        {
                                                            *busy = false;
                                                            *error = e;
                                                        }
                                                    }
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .detach();
                                        cx.notify();
                                    }))
                                }),
                            ),
                    )
            }
        };

        Some(dialog::surface(
            "settings-pop",
            div()
                .id("settings-dialog-scrim")
                .absolute()
                .inset_0()
                .bg(dialog::scrim_color(p.0))
                .flex()
                .items_center()
                .justify_center()
                .p_4(),
            popover_card(p, self.glass_tint)
                .w_full()
                .max_w(px(460.))
                .p_5()
                .child(body),
            460., leaving, cx,
        ))
    }
}
