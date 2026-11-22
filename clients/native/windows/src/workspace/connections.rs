use super::*;
use gpui_component::scroll::ScrollableElement;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const CLOSING_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowState {
    Live,
    Closing,
    Ended,
}

#[derive(Clone)]
struct ConnRow {
    conn: Value,
    state: RowState,
}

#[derive(Default)]
pub(super) struct ConnectionUi {
    hovered: bool,
    frozen: Option<Vec<(String, Value)>>,
    frozen_key: (bool, usize, String),
    closing: HashMap<String, Instant>,
}

impl ConnectionUi {
    pub(super) fn reset(&mut self) {
        self.hovered = false;
        self.frozen = None;
        self.closing.clear();
    }

    pub(super) fn set_hovered(&mut self, hovered: bool) -> bool {
        if self.hovered == hovered {
            return false;
        }
        self.hovered = hovered;
        if !hovered {
            self.frozen = None;
        }
        true
    }

    fn clear_hover(&mut self) {
        self.hovered = false;
        self.frozen = None;
    }

    pub(super) fn mark_closing(&mut self, ids: impl IntoIterator<Item = String>) {
        let now = Instant::now();
        for id in ids {
            if !id.is_empty() {
                self.closing.insert(id, now);
            }
        }
    }

    pub(super) fn unmark_closing<'a>(&mut self, ids: impl IntoIterator<Item = &'a str>) {
        for id in ids {
            self.closing.remove(id);
        }
    }

    pub(super) fn clear_closing(&mut self) {
        self.closing.clear();
    }

    pub(super) fn expire_closing(&mut self) -> bool {
        let before = self.closing.len();
        self.closing.retain(|_, at| at.elapsed() < CLOSING_TIMEOUT);
        self.closing.len() != before
    }

    pub(super) fn prune_closing(&mut self, connections: &Value) {
        if self.closing.is_empty() {
            return;
        }
        let present: HashSet<&str> = connections
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["id"].as_str())
            .collect();
        self.closing.retain(|id, _| present.contains(id.as_str()));
    }

    pub(super) fn is_closing(&self, id: &str) -> bool {
        self.closing
            .get(id)
            .is_some_and(|at| at.elapsed() < CLOSING_TIMEOUT)
    }

    fn prepare_rows(&mut self, live: Vec<Value>, key: (bool, usize, String)) -> Vec<ConnRow> {
        if !self.hovered {
            self.frozen = None;
        } else if self.frozen.is_none() || self.frozen_key != key {
            self.frozen = Some(
                live.iter()
                    .map(|c| (c["id"].as_str().unwrap_or("").to_owned(), c.clone()))
                    .collect(),
            );
            self.frozen_key = key;
        }

        let Some(frozen) = &self.frozen else {
            return live
                .into_iter()
                .map(|conn| ConnRow {
                    conn,
                    state: RowState::Live,
                })
                .collect();
        };

        let mut pending: HashMap<String, Value> = live
            .iter()
            .map(|c| (c["id"].as_str().unwrap_or("").to_owned(), c.clone()))
            .collect();
        let mut rows = Vec::with_capacity(frozen.len().max(live.len()));
        for (id, snapshot) in frozen {
            if let Some(conn) = pending.remove(id) {
                rows.push(ConnRow {
                    conn,
                    state: RowState::Live,
                });
            } else {
                let state = if self.is_closing(id) {
                    RowState::Closing
                } else {
                    RowState::Ended
                };
                rows.push(ConnRow {
                    conn: snapshot.clone(),
                    state,
                });
            }
        }
        for conn in live {
            let id = conn["id"].as_str().unwrap_or("");
            if pending.remove(id).is_some() {
                rows.push(ConnRow {
                    conn,
                    state: RowState::Live,
                });
            }
        }
        rows
    }
}

fn field(row: &Value, key: &str) -> String {
    match &row[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub(super) fn matches_filter(row: &Value, query: &str) -> bool {
    let m = &row["metadata"];
    query.is_empty()
        || [
            "host",
            "sniffHost",
            "destinationIP",
            "sourceIP",
            "destinationPort",
            "network",
            "process",
            "processPath",
        ]
        .iter()
        .any(|k| field(m, k).to_lowercase().contains(query))
        || ["rule", "rulePayload"]
            .iter()
            .any(|k| field(row, k).to_lowercase().contains(query))
        || row["chains"].as_array().is_some_and(|chain| {
            chain
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
                .contains(query)
        })
}

fn address(host: &str, port: &Value) -> String {
    let port = port
        .as_str()
        .map(str::to_owned)
        .or_else(|| port.as_u64().map(|p| p.to_string()))
        .unwrap_or_default();
    if port.is_empty() {
        host.into()
    } else if host.is_empty() {
        format!("端口 {port}")
    } else if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn fmt_duration(row: &Value, closed: bool) -> String {
    let start = row["startMs"].as_u64().unwrap_or(0);
    if start == 0 {
        return String::new();
    }
    let end = if closed {
        row["closedAt"].as_u64().unwrap_or(start)
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(start, |d| d.as_millis() as u64)
    };
    let secs = end.saturating_sub(start) / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

impl Workspace {
    pub(super) fn connection_sort(&self) -> usize {
        if self.history && matches!(self.sort_key, 2 | 3) {
            0
        } else {
            self.sort_key
        }
    }

    pub(super) fn connections_page(&mut self, cx: &mut Context<Self>) -> Div {
        let palette = crate::theme::Palette::get(cx);
        let translucent = true;
        let tint = self.glass_tint;
        let query = self
            .filter
            .as_ref()
            .map(|f| f.read(cx).text.trim().to_lowercase())
            .unwrap_or_default();
        let live_rows: Vec<Value> = self
            .indices(&query)
            .into_iter()
            .filter(|&i| i < self.len())
            .map(|i| self.connection(i).clone())
            .filter(|r| !self.conn_ui.is_closing(r["id"].as_str().unwrap_or("")))
            .collect();
        let rows = self
            .conn_ui
            .prepare_rows(live_rows, (self.history, self.connection_sort(), query.clone()));
        if rows.is_empty() {
            self.conn_ui.clear_hover();
        }
        let active_count = self
            .shown_active()
            .iter()
            .filter(|r| {
                !self.conn_ui.is_closing(r["id"].as_str().unwrap_or(""))
                    && matches_filter(r, &query)
            })
            .count();
        let closed_count = self
            .shown_closed()
            .iter()
            .filter(|r| matches_filter(r, &query))
            .count();
        let total = |key: &str| {
            fmt_bytes(
                self.shown_snapshot()
                    .and_then(|v| v[key].as_u64())
                    .unwrap_or(0),
            )
        };
        let paused = self.frozen_connections.is_some();

        let tabs = div()
            .relative()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded_full()
            .bg(palette.fill())
            .border_1()
            .border_color(palette.control_rim())
            .child(crate::glass::control_layer(palette, px(999.), true))
            .child(
                div()
                    .id("conn-tab-active")
                    .relative()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .px_3()
                    .rounded_full()
                    .text_xs()
                    .when(!self.history, |el| {
                        el.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(self.history, |el| {
                        el.text_color(palette.ink().opacity(0.72))
                    })
                    .child(format!("活跃 ({active_count})"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.history {
                            this.apply(Action::History, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("conn-tab-closed")
                    .relative()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .px_3()
                    .rounded_full()
                    .text_xs()
                    .when(self.history, |el| {
                        el.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(!self.history, |el| {
                        el.text_color(palette.ink().opacity(0.72))
                    })
                    .child(format!("已关闭 ({closed_count})"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.history {
                            this.apply(Action::History, cx);
                        }
                    })),
            );

        let mut actions = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .relative()
                    .overflow_hidden()
                    .h(px(28.))
                    .px_3()
                    .rounded_full()
                    .bg(palette.fill())
                    .border_1()
                    .border_color(palette.control_rim())
                    .shadow_sm()
                    .flex()
                    .items_center()
                    .child(crate::glass::control_layer(palette, px(999.), true))
                    .child(
                        div()
                            .relative()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.ink())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(2.))
                                    .child(div().text_color(palette.muted()).child("↓"))
                                    .child(total("downloadTotal")),
                            )
                            .child(div().text_color(palette.muted()).child("·"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(2.))
                                    .child(div().text_color(palette.muted()).child("↑"))
                                    .child(total("uploadTotal")),
                            ),
                    ),
            )
            .child(
                playback_button("conn-pause", paused, false, palette, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.apply(Action::Pause, cx))),
            );
        if !self.history && active_count > 0 {
            let cmd = if query.is_empty() {
                Command::DisconnectAll
            } else {
                Command::DisconnectMany {
                    ids: rows
                        .iter()
                        .filter(|r| r.state == RowState::Live)
                        .filter_map(|r| r.conn["id"].as_str().map(str::to_owned))
                        .collect(),
                }
            };
            actions = actions.child(
                div()
                    .id("conn-disconnect-all")
                    .relative()
                    .overflow_hidden()
                    .cursor_pointer()
                    .rounded_full()
                    .bg(palette.danger_soft())
                    .border_1()
                    .border_color(palette.control_rim())
                    .shadow_sm()
                    .child(crate::glass::control_layer(palette, px(999.), false))
                    .child(
                        div()
                            .relative()
                            .px_3()
                            .py(px(5.))
                            .rounded_full()
                            .bg(palette.danger_soft())
                            .hover(|s| s.bg(palette.danger_soft()))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.danger())
                            .child(if query.is_empty() {
                                "断开全部".to_owned()
                            } else {
                                format!("断开筛选结果 ({active_count})")
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apply(Action::Confirm(cmd.clone()), cx);
                    })),
            );
        }

        let header = div()
            .w_full()
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
                        crate::glass::backdrop_heading("连接", palette, cx)
                            .text_size(crate::theme::text_px(26.))
                            .line_height(rems(32. / 16.))
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(tabs),
            )
            .child(actions);

        let selected = self.connection_sort();
        let history = self.history;
        let weak = cx.weak_entity();
        let mut picker = DropdownBox::new("conn-sort", SORTS[selected].1, palette)
            .anchor(Anchor::TopRight)
            .width(px(102.))
            .padding_x(px(10.))
            .gap(px(4.));
        for (i, (_, label)) in SORTS.iter().enumerate() {
            if history && matches!(i, 2 | 3) {
                continue;
            }
            let target = weak.clone();
            picker = picker.item(*label, i == selected, move |_, cx| {
                let _ = target.update(cx, |this, cx| this.apply(Action::ConnectionSort(i), cx));
            });
        }
        let toolbar = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(div().flex_1().min_w_0().children(self.filter.clone()))
            .child(div().flex().items_center().gap_2().child(picker));
        let count = rows.len();
        let closed_view = self.history;
        let list_card = glass_card(palette, translucent, tint)
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .py_1()
            .child(if count == 0 {
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(palette.muted())
                    .child(if query.is_empty() {
                        if closed_view {
                            "暂无已关闭的连接"
                        } else {
                            "暂无活跃连接"
                        }
                    } else {
                        "没有找到匹配的连接"
                    })
                    .into_any_element()
            } else {
                div()
                    .id("conn-list-hover")
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if this.conn_ui.set_hovered(*hovered) {
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .relative()
                            .top(self.overscroll_offset(scrolling::ScrollTarget::PageList))
                            .size_full()
                            .child(
                                uniform_list(
                            "connections-list",
                            count,
                            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                                let p = crate::theme::Palette::get(cx);
                                range
                                    .filter_map(|idx| rows.get(idx).map(|row| (idx, row)))
                                    .map(|(idx, item)| {
                                        let r = &item.conn;
                                        let state = item.state;
                                        let is_live = state == RowState::Live;
                                        let m = &r["metadata"];
                                        let host = dashboard::host(r);
                                        let target = address(host, &m["destinationPort"]);
                                        let net = m["network"].as_str().unwrap_or("").to_uppercase();
                                        let kind = m["type"]
                                            .as_str()
                                            .unwrap_or("")
                                            .split('/')
                                            .next()
                                            .unwrap_or("")
                                            .to_uppercase();
                                        let badge = if !kind.is_empty() && !net.is_empty() {
                                            format!("{kind} │ {net}")
                                        } else if !net.is_empty() {
                                            net.clone()
                                        } else {
                                            kind.clone()
                                        };
                                        let is_udp = net == "UDP";
                                        let process = m["process"].as_str().unwrap_or("");
                                        let rule = r["rule"]
                                            .as_str()
                                            .filter(|s| !s.is_empty())
                                            .unwrap_or("final");
                                        let rule_line = rule.replace("rule_set=", "");
                                        let chains = r["chains"]
                                            .as_array()
                                            .map(|arr| {
                                                arr.iter()
                                                    .filter_map(Value::as_str)
                                                    .rev()
                                                    .collect::<Vec<_>>()
                                                    .join(" → ")
                                            })
                                            .unwrap_or_default();
                                        let dl = fmt_bytes(r["download"].as_u64().unwrap_or(0));
                                        let ul = fmt_bytes(r["upload"].as_u64().unwrap_or(0));
                                        let dl_spd = r["downloadSpeed"].as_u64().unwrap_or(0);
                                        let ul_spd = r["uploadSpeed"].as_u64().unwrap_or(0);
                                        let dur = fmt_duration(r, closed_view);
                                        let id_str = r["id"].as_str().unwrap_or("").to_owned();
                                        let row_detail = r.clone();

                                        let mut row = div()
                                            .id(("conn-row", idx))
                                            .w_full()
                                            .h(px(72.))
                                            .px_4()
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .mx_1()
                                            .rounded(px(12.))
                                            .when(!is_live, |el| el.opacity(0.5))
                                            .hover(|s| s.bg(p.hover()))
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.apply(Action::Detail(row_detail.clone()), cx);
                                            }))
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
                                                            .h(px(20.))
                                                            .gap(px(6.))
                                                            .min_w_0()
                                                            .when(!badge.is_empty(), |el| {
                                                                el.child(
                                                                    div()
                                                                        .flex_none()
                                                                        .px(px(6.))
                                                                        .rounded_full()
                                                                        .bg(if is_udp {
                                                                            p.udp_soft()
                                                                        } else {
                                                                            p.tcp_soft()
                                                                        })
                                                                        .text_xs()
                                                                        .line_height(px(20.))
                                                                        .font_weight(FontWeight::MEDIUM)
                                                                        .text_color(if is_udp {
                                                                            p.udp()
                                                                        } else {
                                                                            p.tcp()
                                                                        })
                                                                        .child(badge),
                                                                )
                                                            })
                                                            .child(
                                                                div()
                                                                    .text_sm()
                                                                    .line_height(px(20.))
                                                                    .truncate()
                                                                    .child(target),
                                                            )
                                                            .when(!process.is_empty(), |el| {
                                                                el.child(
                                                                    div()
                                                                        .flex_none()
                                                                        .text_xs()
                                                                        .text_color(p.muted())
                                                                        .child(format!("· {process}")),
                                                                )
                                                            }),
                                                    )
                                                    .when(!rule_line.is_empty(), |el| {
                                                        el.child(
                                                            div()
                                                                .text_xs()
                                                                .line_height(px(16.))
                                                                .text_color(p.muted())
                                                                .truncate()
                                                                .child(rule_line),
                                                        )
                                                    })
                                                    .when(!chains.is_empty(), |el| {
                                                        el.child(
                                                            div()
                                                                .text_xs()
                                                                .line_height(px(16.))
                                                                .text_color(p.ink().opacity(0.80))
                                                                .truncate()
                                                                .child(crate::emoji::text(chains)),
                                                        )
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .w(px(208.))
                                                    .flex_none()
                                                    .flex()
                                                    .flex_col()
                                                    .items_end()
                                                    .gap(px(2.))
                                                    .when(!is_live, |el| {
                                                        el.child(
                                                            div()
                                                                .text_xs()
                                                                .font_weight(FontWeight::MEDIUM)
                                                                .text_color(p.ink())
                                                                .child(if state == RowState::Closing {
                                                                    "正在断开…"
                                                                } else {
                                                                    "已结束"
                                                                }),
                                                        )
                                                    })
                                                    .when(is_live && !closed_view, |el| {
                                                        el.child(
                                                            div()
                                                                .text_xs()
                                                                .font_weight(FontWeight::MEDIUM)
                                                                .text_color(p.ink())
                                                                .child(format!(
                                                                    "↓ {}/s · ↑ {}/s",
                                                                    fmt_bytes(dl_spd),
                                                                    fmt_bytes(ul_spd)
                                                                )),
                                                        )
                                                    })
                                                    .child(div().text_xs().text_color(p.muted()).child(
                                                        if closed_view {
                                                            format!("↓ {dl} · ↑ {ul}")
                                                        } else {
                                                            format!("{dl} / {ul} · {dur}")
                                                        },
                                                    ))
                                                    .when(closed_view && !dur.is_empty(), |el| {
                                                        el.child(
                                                            div()
                                                                .text_size(crate::theme::text_px(11.))
                                                                .text_color(p.muted())
                                                                .child(dur),
                                                        )
                                                    }),
                                            );
                                        if !closed_view {
                                            row = row.child(
                                                div()
                                                    .id(("conn-close", idx))
                                                    .size(px(24.))
                                                    .flex_none()
                                                    .rounded_full()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_xs()
                                                    .text_color(p.muted())
                                                    .when(is_live, |btn| {
                                                        btn.cursor_pointer()
                                                            .hover(|s| {
                                                                s.bg(p.danger_soft())
                                                                    .text_color(p.danger())
                                                            })
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    cx.stop_propagation();
                                                                    this.apply(
                                                                        Action::Run(
                                                                            Command::Disconnect {
                                                                                id: id_str.clone(),
                                                                            },
                                                                        ),
                                                                        cx,
                                                                    );
                                                                },
                                                            ))
                                                    })
                                                    .child(
                                                        svg()
                                                            .path("x-small")
                                                            .size(px(12.))
                                                            .text_color(p.danger()),
                                                    ),
                                            );
                                        }
                                        row
                                    })
                                    .collect()
                            }),
                        )
                        .track_scroll(&self.page_list_scroll)
                        .size_full(),
                    ),
                )
                    .child(self.smooth_scroll_capture(scrolling::ScrollTarget::PageList, cx))
                    .into_any_element()
            })
            .when(count > 0, |card| card.vertical_scrollbar(&self.page_list_scroll));

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(header)
            .child(toolbar)
            .when(!self.load_error.is_empty(), |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(palette.muted())
                        .child(self.load_error.clone()),
                )
            })
            .child(list_card)
    }

    pub(crate) fn connection_overlay(&self, window: &Window, cx: &Context<Self>) -> Option<Div> {
        let mut detail = self.detail.as_ref();
        let mut confirm = self.confirm.as_ref();
        let leaving = match &self.leaving_dialog {
            Some((_, dialog::LeavingDialog::Connection(row))) if detail.is_none() => {
                detail = Some(row);
                true
            }
            Some((_, dialog::LeavingDialog::Disconnect(command))) if confirm.is_none() => {
                confirm = Some(command);
                true
            }
            _ => false,
        };
        if detail.is_none() && confirm.is_none() {
            return None;
        }
        let tint = self.glass_tint;
        let palette = crate::theme::Palette::get(cx);
        let query = self
            .filter
            .as_ref()
            .map(|f| f.read(cx).text.trim().to_lowercase())
            .unwrap_or_default();
        let active_count = self
            .shown_active()
            .iter()
            .filter(|r| matches_filter(r, &query))
            .count();
        let modal_height = f32::from(window.viewport_size().height) * 0.8;
        Some(div().absolute().inset_0().size_full()
            .when_some(detail, |el, r| {
                let current = self.shown_active().iter().find(|v| v["id"] == r["id"]);
                let open = current.is_some();
                let r = current.or_else(|| self.shown_closed().iter().find(|v| v["id"] == r["id"])).unwrap_or(r);
                let m = &r["metadata"];
                let ip = field(m, "destinationIP");
                let section = |title: &'static str, fields: Vec<(&'static str, String)>| {
                    div().flex().flex_col().gap_1().child(div().mb_1().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(palette.muted()).child(title))
                        .children(fields.into_iter().map(|(key, value)| div().flex().gap_3().text_sm()
                            .child(div().w(px(90.)).flex_none().text_color(palette.muted()).child(key))
                            .child(div().flex_1().min_w_0().child(crate::emoji::text(if value.is_empty() { "—".into() } else { value })))))
                };
                let mut basic = vec![("ID", field(r,"id")), ("网络", field(m,"network").to_uppercase()), ("入站", field(m,"type")),
                    ("主机", dashboard::host(r).to_owned()), ("规则", r["rule"].as_str().filter(|s| !s.is_empty()).unwrap_or("final").to_owned())];
                if let Some(payload) = r["rulePayload"].as_str().filter(|s| !s.is_empty()) { basic.push(("规则载荷", payload.into())); }
                basic.extend([("链路", r["chains"].as_array().into_iter().flatten().rev().filter_map(Value::as_str).collect::<Vec<_>>().join(" → ")),
                    ("开始时间", field(r, "start")), ("持续时长", fmt_duration(r, !open))]);
                let mut network = vec![("源地址",address(&field(m,"sourceIP"), &m["sourcePort"])), ("目标地址",address(&ip,&m["destinationPort"])), ("DNS 模式",field(m,"dnsMode"))];
                if !field(m,"sniffHost").is_empty() { network.push(("嗅探主机", field(m,"sniffHost"))); }
                network.push(("进程", field(m,"process")));
                if !field(m,"processPath").is_empty() { network.push(("进程路径", field(m,"processPath"))); }
                let mut traffic = vec![("下载总量", fmt_bytes(r["download"].as_u64().unwrap_or(0))), ("上传总量", fmt_bytes(r["upload"].as_u64().unwrap_or(0)))];
                if open { traffic.extend([("下载速度", format!("{}/s",fmt_bytes(r["downloadSpeed"].as_u64().unwrap_or(0)))),
                    ("上传速度",format!("{}/s",fmt_bytes(r["uploadSpeed"].as_u64().unwrap_or(0))))]); }
                let header = div().flex().items_center().justify_between().gap_2()
                    .child(div().text_lg().font_weight(FontWeight::BOLD).truncate().child(format!("{}{}",dashboard::host(r),if open { "" } else { "  已关闭" })))
                    .child(dialog::keyboard_action(div().id("conn-detail-close").cursor_pointer().p_2().child(svg().path("close-detail").size(px(12.)).text_color(palette.muted()))
                        .on_click(cx.listener(|this,_,_,cx| this.apply(Action::Dismiss,cx))), Action::Dismiss, cx));
                let mut body = div()
                    .relative()
                    .top(self.overscroll_offset(scrolling::ScrollTarget::ConnDetail))
                    .w_full()
                    .min_h_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                        if this.handle_smooth_wheel_for(scrolling::ScrollTarget::ConnDetail, event)
                        {
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .child(section("基本信息", basic))
                    .child(section("网络信息", network));
                if singboard_stream::tools::public_ip(&ip) {
                    body = body.child(match &self.geo {
                        Some(Ok(v)) => {
                            let provider = ["asn_organization", "organization", "isp"]
                                .iter()
                                .filter_map(|key| v[key].as_str().map(str::trim))
                                .find(|s| !s.is_empty())
                                .unwrap_or("");
                            let mut loc_parts: Vec<&str> = Vec::new();
                            for key in ["city", "region", "country"] {
                                if let Some(part) = v[key].as_str().map(str::trim).filter(|s| !s.is_empty())
                                    && !loc_parts.iter().any(|existing| existing.eq_ignore_ascii_case(part))
                                {
                                    loc_parts.push(part);
                                }
                            }
                            section("IP 地理信息",vec![("IP",ip.clone()),("ASN", if v["asn"].is_null() {String::new()} else {format!("AS{}",field(v,"asn"))}),
                                ("运营商",provider.into()),
                                ("位置",loc_parts.join(", "))])
                        }
                        Some(Err(e)) => section("IP 地理信息",vec![("查询失败",e.clone())]).child(self.button("conn-geo-retry","重试",Action::Geo(ip.clone()),cx)),
                        None => section("IP 地理信息",vec![("", "正在查询…".into())])
                    });
                }
                let body_pane = div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("conn-detail-body")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.conn_detail_scroll)
                            .child(body.child(section("流量信息", traffic))),
                    )
                    .vertical_scrollbar(&self.conn_detail_scroll);
                let footer = div().flex().justify_end().gap_2().when(open, |el| {
                    let id = field(r, "id");
                    el.child(self.button("conn-detail-disconnect", "断开连接", Action::Run(Command::Disconnect { id }), cx)
                        .text_color(palette.danger()).bg(palette.danger_soft()))
                }).child(self.button("conn-done","完成",Action::Dismiss,cx));
                el.child(dialog::surface("conn-details-pop", self.page_dialog("conn-details", Action::Dismiss, cx), popover_card(palette, tint)
                    .id("conn-dialog-card").w(px(672.)).max_w_full().max_h(px(modal_height)).p_5().gap_3().flex().flex_col()
                    .on_click(|_,_,cx| cx.stop_propagation()).child(header).child(body_pane).child(footer), 672., leaving, cx))
            })
            .when_some(confirm, |el, cmd| {
                let (title, message) = match cmd {
                    Command::DisconnectAll => ("断开全部连接", format!("将断开当前全部 {active_count} 个连接，正在进行的下载、通话和视频都会中断。")),
                    Command::DisconnectMany{ids} => ("断开筛选结果", format!("将断开匹配“{query}”的 {} 个连接。",ids.len())),
                    _ => return el,
                };
                el.child(dialog::surface("conn-confirm-pop", self.page_dialog("conn-confirm", Action::Cancel,cx), popover_card(palette, tint).id("conn-confirm-card")
                    .on_click(|_,_,cx| cx.stop_propagation()).w(px(400.)).max_w_full().p_5().gap_3().flex().flex_col()
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child(title))
                    .child(div().text_sm().child(message)).child(div().flex().justify_end().gap_2()
                        .child(self.button("cancel-disconnect","取消",Action::Cancel,cx))
                        .child(self.button("confirm-disconnect","断开",Action::Run(cmd.clone()),cx).bg(palette.danger_fill()).text_color(rgb(0xffffff)))), 400., leaving, cx))
            }))
    }

    pub(super) fn page_dialog(
        &self,
        id: &'static str,
        close: Action,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let p = crate::theme::Palette::get(cx);
        div()
            .id(id)
            .capture_key_down(cx.listener(|this, _, _, cx| {
                if this.leaving_dialog.is_some() { cx.stop_propagation(); }
            }))
            .absolute()
            .inset_0()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(dialog::scrim_color(p.0))
            .on_click(cx.listener(move |this, _, _, cx| this.apply(close.clone(), cx)))
    }
}
