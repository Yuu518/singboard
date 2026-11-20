use super::*;
use gpui_component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    scroll::ScrollableElement,
};

fn format_num(n: usize) -> String {
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

fn count_lines<'a>(iter: impl Iterator<Item = &'a LogRecord>) -> usize {
    iter.filter(|r| !r.marker).count()
}

impl Workspace {
    fn log_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let query = self.filter.as_ref().map(|f| f.read(cx).text.trim().to_lowercase()).unwrap_or_default();
        let rows = self.indices(&query);
        let source = self.frozen.as_ref().unwrap_or(&self.logs);
        let lines = rows.iter().enumerate().filter_map(|(index, &row)| {
            source.get(row).filter(|line| !line.marker).map(|line| (index, line.seq))
        }).collect::<Vec<_>>();
        if lines.is_empty() { return; }
        let last = lines.len() - 1;
        let current = lines.iter().position(|(_, seq)| Some(*seq) == self.log_active_row).unwrap_or(last);
        let next = match event.keystroke.key.as_str() {
            "up" => current.saturating_sub(1),
            "down" => (current + 1).min(last),
            "pageup" => current.saturating_sub(10),
            "pagedown" => (current + 10).min(last),
            "home" => 0,
            "end" => last,
            "enter" | "space" => {
                let (index, seq) = lines[current];
                if !self.expanded_logs.remove(&seq) { self.expanded_logs.insert(seq); }
                self.log_list.remeasure_items(index..index + 1);
                current
            }
            _ => return,
        };
        let (index, seq) = lines[next];
        self.log_active_row = Some(seq);
        self.log_list.scroll_to_reveal_item(index);
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn logs_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let palette = crate::theme::Palette::get(cx);
        let translucent = true;
        let tint = self.glass_tint;
        let narrow = f32::from(window.viewport_size().width) < 640.;

        let live = self.log_status == "已连接";
        if !self.log_scroll_bound {
            self.log_scroll_bound = true;
            let weak = cx.weak_entity();
            self.log_list.set_scroll_handler(move |event, _, cx| {
                let following = event.is_following_tail;
                let _ = weak.update(cx, |this, cx| {
                    if this.log_auto_scroll != following {
                        this.log_auto_scroll = following;
                        cx.notify();
                    }
                });
            });
        }

        let query = self
            .filter
            .as_ref()
            .map(|f| f.read(cx).text.trim().to_lowercase())
            .unwrap_or_default();
        let rows = self.indices(&query);
        let paused = self.frozen.is_some();
        let source_slice = self.frozen.as_ref().unwrap_or(&self.logs);
        let total_lines = count_lines(source_slice.iter());
        let shown_lines = rows
            .iter()
            .filter(|&&i| i < source_slice.len() && !source_slice[i].marker)
            .count();
        let shown = rows.len();

        let filter_key = (query.clone(), self.log_level, self.core_level.clone(), paused);
        let row_focus = self.log_focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let head_seq = rows
            .first()
            .and_then(|&i| source_slice.get(i))
            .map(|r| r.seq);

        if filter_key != self.log_list_filter || shown == 0 || self.log_list_len == 0 {
            self.log_list.reset_with_uniform_height(shown, px(26.));
            self.log_auto_scroll = true;
            self.log_list.set_follow_mode(FollowMode::Tail);
        } else if head_seq == self.log_list_head {
            if shown > self.log_list_len {
                self.log_list
                    .splice(self.log_list_len..self.log_list_len, shown - self.log_list_len);
            } else if shown < self.log_list_len {
                self.log_list.reset_with_uniform_height(shown, px(26.));
                self.log_auto_scroll = true;
                self.log_list.set_follow_mode(FollowMode::Tail);
            }
        } else {
            self.log_list.reset_with_uniform_height(shown, px(26.));
            if self.log_auto_scroll {
                self.log_list.set_follow_mode(FollowMode::Tail);
            }
        }
        self.log_list_len = shown;
        self.log_list_head = head_seq;
        self.log_list_filter = filter_key;

        let count_text = if shown_lines == total_lines {
            format_num(total_lines)
        } else {
            format!("{} / {}", format_num(shown_lines), format_num(total_lines))
        };

        let effective_level = self.effective_log_level();
        let level_filtered = effective_level != self.core_level;
        let level_label = if self.log_level == 0 {
            format!("跟随核心（{}）", self.core_level)
        } else {
            LEVELS[self.log_level].to_owned()
        };

        let selected_level = self.log_level;
        let core_lvl = self.core_level.clone();
        let view_weak = cx.weak_entity();
        let level_picker = div()
            .relative()
            .overflow_hidden()
            .h(px(32.))
            .min_w(px(138.))
            .rounded(px(10.))
            .bg(palette.fill())
            .border_1()
            .border_color(palette.rim())
            .child(crate::glass::control_layer(palette, px(9.), false))
            .child(
                Button::new("logs-level")
                    .ghost()
                    .w_full()
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(10.))
                    .text_color(palette.ink())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(div().min_w_0().truncate().text_size(px(13.)).child(level_label))
                            .child(
                                svg()
                                    .path("select-caret")
                                    .size(px(15.))
                                    .text_color(palette.muted()),
                            ),
                    )
                    .dropdown_menu(move |mut menu, _, _| {
                        for (idx, name) in LEVELS.iter().enumerate() {
                            let label = if idx == 0 {
                                format!("跟随核心（{core_lvl}）")
                            } else {
                                (*name).to_owned()
                            };
                            let view = view_weak.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label)
                                    .checked(idx == selected_level)
                                    .on_click(move |_, _, cx| {
                                        let _ = view.update(cx, |v, cx| v.set_log_level(idx, cx));
                                    }),
                            );
                        }
                        menu
                    }),
            );

        let cleared_lines = count_lines(self.cleared.iter());
        let show_cleared = cleared_lines > 0
            && self
                .cleared_at
                .is_some_and(|t| t.elapsed().as_secs() < 8);
        let api_unreachable = !live && self.log_failures >= 3;
        let show_reconnecting = !paused && !show_cleared && !live && total_lines > 0;

        let title_block = div()
            .flex()
            .items_baseline()
            .gap_2()
            .child(
                crate::glass::backdrop_heading("日志", palette, cx)
                    .text_size(px(26.))
                    .line_height(px(32.))
                    .font_weight(FontWeight::BOLD),
            )
            .child(
                crate::glass::backdrop_text(count_text, true, palette, cx).text_sm(),
            );

        let clear_disabled = total_lines == 0 && !paused;
        let actions_block = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                playback_button("logs-pause", paused, false, palette, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.apply(Action::Pause, cx))),
            )
            .child(
                div()
                    .id("logs-clear")
                    .relative()
                    .overflow_hidden()
                    .h(px(32.))
                    .rounded_full()
                    .bg(palette.fill())
                    .border_1()
                    .border_color(palette.rim())
                    .when(!clear_disabled, |d| d.cursor_pointer().shadow_sm())
                    .when(clear_disabled, |d| d.opacity(0.55))
                    .child(crate::glass::control_layer(palette, px(999.), true))
                    .child(
                        div()
                            .relative()
                            .h_full()
                            .px_3()
                            .rounded_full()
                            .when(!clear_disabled, |d| d.hover(|s| s.bg(palette.fill_strong())))
                            .flex()
                            .items_center()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if clear_disabled {
                                palette.muted()
                            } else {
                                palette.ink()
                            })
                            .child("清空"),
                    )
                    .when(!clear_disabled, |d| {
                        d.on_click(cx.listener(|this, _, _, cx| this.apply(Action::Clear, cx)))
                    }),
            );

        let toolbar = if narrow {
            div()
                .w_full()
                .flex_none()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(title_block)
                        .child(actions_block),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().min_w_0().children(self.filter.clone()))
                        .child(div().flex_none().max_w(px(176.)).child(level_picker)),
                )
        } else {
            div()
                .w_full()
                .flex_none()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(title_block)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(level_picker)
                                .child(actions_block),
                        ),
                )
                .child(div().w_full().children(self.filter.clone()))
        };

        let banner = if paused {
            Some(
                glass_card(palette, translucent, tint)
                    .w_full()
                    .flex_none()
                    .px_3()
                    .py(px(6.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(if self.paused_count > 0 {
                        format!("已暂停刷新，暂停后收到 {} 条新日志。", self.paused_count)
                    } else {
                        "已暂停刷新。".into()
                    })
                    .child(
                        div()
                            .id("logs-resume-banner")
                            .cursor_pointer()
                            .px_2()
                            .py(px(2.))
                            .rounded(px(6.))
                            .hover(|s| s.bg(palette.hover()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.accent())
                            .child("继续刷新")
                            .on_click(cx.listener(|this, _, _, cx| this.apply(Action::Pause, cx))),
                    ),
            )
        } else if show_cleared {
            Some(
                glass_card(palette, translucent, tint)
                    .w_full()
                    .flex_none()
                    .px_3()
                    .py(px(6.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(format!("已清空 {cleared_lines} 条日志。"))
                    .child(
                        div()
                            .id("logs-undo-banner")
                            .cursor_pointer()
                            .px_2()
                            .py(px(2.))
                            .rounded(px(6.))
                            .hover(|s| s.bg(palette.hover()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.accent())
                            .child("撤销")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply(Action::UndoClear, cx);
                            })),
                    ),
            )
        } else if show_reconnecting {
            Some(
                glass_card(palette, translucent, tint)
                    .w_full()
                    .flex_none()
                    .px_3()
                    .py(px(6.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(if api_unreachable {
                        "无法连接 Clash API，正在重试；下面是中断前的日志。"
                    } else {
                        "与核心的连接中断，正在重连；下面是中断前的日志。"
                    })
                    .when(api_unreachable, |el| {
                        el.child(
                            div()
                                .id("logs-api-settings")
                                .cursor_pointer()
                                .px_2()
                                .py(px(2.))
                                .rounded(px(6.))
                                .hover(|s| s.bg(palette.hover()))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.accent())
                                .child("检查 API 配置")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open(Page::Settings, cx);
                                })),
                        )
                    }),
            )
        } else {
            None
        };

        let scrolled_up = shown > 0 && !self.log_auto_scroll;

        let list_body: AnyElement = if shown == 0 {
            if (!query.is_empty() || level_filtered) && total_lines > 0 {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .px_6()
                    .child(
                        div()
                            .text_sm()
                            .text_color(palette.ink())
                            .child(if !query.is_empty() {
                                format!("没有匹配“{query}”的日志")
                            } else {
                                format!("当前级别 {effective_level} 下没有日志")
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .when(!query.is_empty(), |row| {
                                row.child(
                                    div()
                                        .id("logs-empty-clear-search")
                                        .cursor_pointer()
                                        .h(px(32.))
                                        .px_5()
                                        .rounded(px(10.))
                                        .bg(palette.fill())
                                        .hover(|s| s.bg(palette.fill_strong()))
                                        .flex()
                                        .items_center()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.accent())
                                        .child("清除搜索")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(f) = &this.filter {
                                                f.update(cx, |s, cx| s.clear(cx));
                                            }
                                            cx.notify();
                                        })),
                                )
                            })
                            .when(level_filtered, |row| {
                                row.child(
                                    div()
                                        .id("logs-empty-reset-level")
                                        .cursor_pointer()
                                        .h(px(32.))
                                        .px_5()
                                        .rounded(px(10.))
                                        .bg(palette.fill())
                                        .hover(|s| s.bg(palette.fill_strong()))
                                        .flex()
                                        .items_center()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.accent())
                                        .child("跟随核心级别")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.set_log_level(0, cx);
                                        })),
                                )
                            }),
                    )
                    .into_any_element()
            } else if !live && api_unreachable {
                div()
                    .size_full()
                    .flex()
                    .justify_center()
                    .pt_6()
                    .px_6()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(520.))
                            .py_5()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("无法连接 Clash API"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(palette.muted())
                                    .child("请确认 sing-box 配置里开启了 clash_api，并且地址和密钥与当前的 Clash API 配置一致。"),
                            )
                            .child(
                                div()
                                    .pt_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .text_xs()
                                    .child(
                                        div()
                                            .flex()
                                            .gap_3()
                                            .child(div().text_color(palette.muted()).child("地址"))
                                            .child(
                                                div()
                                                    .font_family("Consolas")
                                                    .child(if self.endpoint.is_empty() {
                                                        "未配置".into()
                                                    } else {
                                                        self.endpoint.clone()
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap_3()
                                            .child(div().text_color(palette.muted()).child("状态"))
                                            .child(format!(
                                                "已连续 {} 次连接失败，仍在自动重试",
                                                self.log_failures
                                            )),
                                    ),
                            )
                            .child(
                                div().pt_2().flex().child(
                                    div()
                                        .id("logs-empty-api-settings")
                                        .cursor_pointer()
                                        .h(px(32.))
                                        .px_5()
                                        .rounded(px(10.))
                                        .bg(palette.fill())
                                        .hover(|s| s.bg(palette.fill_strong()))
                                        .flex()
                                        .items_center()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette.accent())
                                        .child("检查 API 配置")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.open(Page::Settings, cx);
                                        })),
                                ),
                            ),
                    )
                    .into_any_element()
            } else if !live {
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(palette.muted())
                    .child("正在连接核心…")
                    .into_any_element()
            } else {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .px_6()
                    .child(
                        div()
                            .text_sm()
                            .text_color(palette.ink())
                            .child(if paused {
                                "暂停前没有日志"
                            } else {
                                "已连接，暂时没有日志"
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted())
                            .child(format!(
                                "当前级别 {effective_level}{}，有新日志时会出现在这里。",
                                if self.log_level == 0 {
                                    "（跟随核心）"
                                } else {
                                    ""
                                }
                            )),
                    )
                    .into_any_element()
            }
        } else {
            let entity = cx.entity();
            let list_el = list(self.log_list.clone(), move |idx, window, cx| {
                let p = crate::theme::Palette::get(cx);
                let this = entity.read(cx);
                let source = this.frozen.as_ref().unwrap_or(&this.logs);
                let Some(&src_idx) = rows.get(idx) else {
                    return div().into_any_element();
                };
                let Some(entry) = source.get(src_idx) else {
                    return div().into_any_element();
                };
                if entry.marker {
                    return div()
                        .w_full()
                        .h(px(26.))
                        .px(px(14.))
                        .flex()
                        .items_center()
                        .gap_3()
                        .text_size(px(11.5))
                        .text_color(p.muted())
                        .child(div().flex_1().h(px(0.5)).bg(p.line()))
                        .child(div().flex_none().child(entry.payload.clone()))
                        .child(div().flex_1().h(px(0.5)).bg(p.line()))
                        .into_any_element();
                }
                let seq = entry.seq;
                let expanded = this.expanded_logs.contains(&seq);
                let lvl_low = entry.level.to_lowercase();
                let (pill_bg, pill_fg, row_bg) = match lvl_low.as_str() {
                    "error" | "fatal" | "panic" => {
                        (p.danger_soft(), p.danger(), Some(p.row_error()))
                    }
                    "warn" | "warning" => (p.warning_soft(), p.warning_ink(), Some(p.row_warn())),
                    "info" => (p.fill(), p.ink(), None),
                    _ => (p.fill(), p.muted(), None),
                };
                let label = lvl_low.to_uppercase();
                let copy_text = entry.payload.clone();
                let entity_click = entity.clone();
                div()
                    .id(("log-row", seq))
                    .w_full()
                    .px(px(14.))
                    .flex()
                    .gap(px(10.))
                    .cursor_default()
                    .border_b(px(0.5))
                    .border_color(p.line())
                    .when(expanded, |el| el.items_start().py_1())
                    .when(!expanded, |el| el.h(px(26.)).items_center())
                    .when_some(row_bg, |el, bg| el.bg(bg))
                    .when(this.log_active_row == Some(seq) && this.log_focus.as_ref().is_some_and(|f| f.is_focused(window)), |el| el.bg(p.fill_strong()))
                    .on_click(move |_, window, cx| {
                        entity_click.update(cx, |this, cx| {
                            if let Some(focus) = &this.log_focus { focus.focus(window, cx); }
                            this.log_active_row = Some(seq);
                            if !this.expanded_logs.remove(&seq) {
                                this.expanded_logs.insert(seq);
                            }
                            this.log_list.remeasure_items(idx..idx + 1);
                            cx.notify();
                        });
                    })
                    .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                    })
                    .child(
                        div()
                            .w(px(62.))
                            .flex_none()
                            .font_family("Consolas")
                            .text_size(px(11.5))
                            .line_height(px(18.))
                            .text_color(p.muted())
                            .child(entry.time.clone()),
                    )
                    .child(
                        div()
                            .w(px(52.))
                            .h(px(18.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(pill_bg)
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(pill_fg)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family("Consolas")
                            .text_xs()
                            .line_height(px(18.))
                            .text_color(p.ink())
                            .when(!expanded, |el| el.truncate())
                            .child(entry.payload.clone()),
                    )
                    .into_any_element()
            })
            .size_full()
            .py(px(6.));
            div()
                .relative()
                .size_full()
                .child(list_el)
                .vertical_scrollbar(&self.log_list)
                .into_any_element()
        };

        let list_card = glass_card(palette, translucent, tint)
            .track_focus(&row_focus)
            .tab_index(0)
            .on_key_down(cx.listener(Self::log_key))
            .relative()
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(list_body)
            .when(scrolled_up, |card| {
                card.child(
                    div()
                        .id("logs-scroll-latest")
                        .absolute()
                        .bottom_3()
                        .right_3()
                        .overflow_hidden()
                        .cursor_pointer()
                        .rounded_full()
                        .bg(palette.fill())
                        .border_1()
                        .border_color(palette.rim())
                        .shadow_md()
                        .child(crate::glass::control_layer(palette, px(999.), true))
                        .child(
                            div()
                                .relative()
                                .h(px(32.))
                                .px_3()
                                .rounded_full()
                                .hover(|s| s.bg(palette.fill_strong()))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.accent())
                                .child(
                                    svg()
                                        .path("arrow-down")
                                        .size(px(14.))
                                        .text_color(palette.accent()),
                                )
                                .child("回到最新"),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.log_auto_scroll = true;
                            this.log_list.set_follow_mode(FollowMode::Tail);
                            this.log_list.scroll_to_end();
                            cx.notify();
                        })),
                )
            });

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(toolbar)
            .when_some(banner, |el, card| el.child(card))
            .child(list_card)
    }
}
