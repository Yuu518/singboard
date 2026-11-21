use crate::{editor, service, theme::Palette, visibility, workspace::Workspace};
use gpui::{prelude::*, *};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{
            GWL_STYLE, GetWindowLongW, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_SHOWWINDOW,
            SetForegroundWindow, SetWindowLongW, SetWindowPos, WS_CAPTION, WS_POPUP, WS_THICKFRAME,
        },
    },
};

struct TrayWindow(Option<WindowHandle<TrayMenu>>);
impl Global for TrayWindow {}

pub fn init(cx: &mut App) {
    cx.set_global(TrayWindow(None));
}

struct TrayMenu {
    panel: AnyWindowHandle,
    workspace: Entity<Workspace>,
    focus: FocusHandle,
    selected: Option<usize>,
    busy: Option<usize>,
    error: String,
}

fn place(window: &Window, x: i32, y: i32) -> bool {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return false;
    };
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return false;
        }
        let (mut dx, mut dy) = (96, 96);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) < 0 {
            dx = 96;
            dy = 96;
        }
        let screen = info.rcWork;
        let w = ((144 * dx + 48) / 96) as i32;
        let h = ((132 * dy + 48) / 96) as i32;
        let left = if x + w <= screen.right { x } else { x - w };
        let top = if y - h >= screen.top { y - h } else { y };
        let hwnd = handle.hwnd.get() as _;
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        SetWindowLongW(
            hwnd,
            GWL_STYLE,
            ((style & !(WS_CAPTION | WS_THICKFRAME)) | WS_POPUP) as i32,
        );
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            left.clamp(screen.left, (screen.right - w).max(screen.left)),
            top.clamp(screen.top, (screen.bottom - h).max(screen.top)),
            w,
            h,
            SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        );
        SetForegroundWindow(hwnd) != 0
    }
}

pub fn open(x: i32, y: i32, panel: AnyWindowHandle, workspace: Entity<Workspace>, cx: &mut App) {
    let existing = cx.global::<TrayWindow>().0;
    if let Some(handle) = existing
        && handle
            .update(cx, |menu, window, cx| {
                menu.present(x, y, window, cx);
            })
            .is_ok()
    {
        return;
    }
    let bounds = Bounds::centered(None, size(px(144.), px(132.)), cx);
    if let Ok(handle) = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: WindowBackgroundAppearance::Transparent,
            kind: WindowKind::PopUp,
            titlebar: None,
            show: false,
            focus: false,
            is_resizable: false,
            is_movable: false,
            is_minimizable: false,
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| {
                cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
                cx.observe_window_activation(window, |_, window, _| {
                    if !window.is_window_active() {
                        visibility(window, false);
                    }
                })
                .detach();
                TrayMenu {
                    panel,
                    workspace,
                    focus: cx.focus_handle(),
                    selected: None,
                    busy: None,
                    error: String::new(),
                }
            })
        },
    ) {
        cx.global_mut::<TrayWindow>().0 = Some(handle);
        let _ = handle.update(cx, |menu, window, cx| menu.present(x, y, window, cx));
    }
}

impl TrayMenu {
    fn present(&mut self, x: i32, y: i32, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = None;
        self.error.clear();
        self.workspace
            .update(cx, |ws, cx| ws.refresh_local_state(cx));
        let focused = place(window, x, y);
        window.focus(&self.focus, cx);
        if !focused {
            let handle = gpui::Window::window_handle(window);
            cx.spawn(async move |_, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(350))
                    .await;
                let _ = handle.update(cx, |_, window, _| {
                    if !window.is_window_active() {
                        visibility(window, false);
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        match index {
            2 => {
                visibility(window, false);
                let _ = self.panel.update(cx, |_, w, _| visibility(w, true));
            }
            3 => {
                visibility(window, false);
                editor::quit(cx);
            }
            _ if self.busy.is_some() => {}
            _ => {
                let action = if index == 1 {
                    "restart"
                } else if self.workspace.read(cx).local_state.contains("运行中") {
                    "stop"
                } else {
                    "start"
                };
                self.error.clear();
                self.busy = Some(index);
                let task = singboard_stream::runtime().spawn(service::control(action));
                cx.spawn(async move |weak, cx| {
                    let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
                    let _ = weak.update(cx, |menu, cx| {
                        menu.busy = None;
                        if let Err(error) = result
                            && error != "已取消管理员授权"
                        {
                            menu.error = error;
                        }
                        menu.workspace
                            .update(cx, |ws, cx| ws.refresh_local_state(cx));
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }
}

impl Render for TrayMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let running = self.workspace.read(cx).local_state.contains("运行中");
        let item = |i: usize, label: &'static str| {
            div()
                .id(("tray-item", i))
                .role(Role::Button)
                .aria_label(label)
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .when(self.selected == Some(i), |e| {
                    e.border_1().border_color(p.accent())
                })
                .on_click(cx.listener(move |this, _, window, cx| this.activate(i, window, cx)))
        };
        let toggle = if running {
            "停止服务"
        } else {
            "启动服务"
        };
        let actions = div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                item(0, toggle)
                    .size(px(32.))
                    .rounded_full()
                    .shadow_sm()
                    .bg(if running {
                        p.success_dot()
                    } else {
                        p.danger_fill()
                    })
                    .when(self.busy == Some(0), |e| e.child("…"))
                    .when(self.busy != Some(0), |e| {
                        e.child(
                            svg()
                                .path("tray-power")
                                .size(px(16.))
                                .text_color(rgb(0xffffff)),
                        )
                    }),
            )
            .child(
                item(1, "重启服务")
                    .size(px(32.))
                    .rounded_full()
                    .bg(p.fill())
                    .hover(|s| s.bg(p.fill_strong()))
                    .when(self.busy == Some(1), |e| e.child("…"))
                    .when(self.busy != Some(1), |e| {
                        e.child(svg().path("tray-restart").size(px(16.)).text_color(p.ink()))
                    }),
            );
        div()
            .size_full()
            .font_family(crate::theme::system_ui_font())
            .p(px(6.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, w, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => visibility(w, false),
                    "tab" | "up" | "down" | "left" | "right" => {
                        let back = event.keystroke.modifiers.shift
                            || matches!(event.keystroke.key.as_str(), "up" | "left");
                        this.selected = Some(this.selected.map_or(if back { 3 } else { 0 }, |i| {
                            if back { (i + 3) % 4 } else { (i + 1) % 4 }
                        }));
                    }
                    "enter" | "space" => {
                        if let Some(i) = this.selected {
                            this.activate(i, w, cx);
                        }
                    }
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                div()
                    .size_full()
                    .rounded(px(18.))
                    .p_1()
                    .gap(px(2.))
                    .flex()
                    .flex_col()
                    .bg(p.card(false))
                    .border_1()
                    .border_color(p.rim())
                    .shadow_sm()
                    .text_color(p.ink())
                    .child(actions)
                    .when(!self.error.is_empty(), |e| {
                        e.child(
                            div()
                                .px_2()
                                .truncate()
                                .text_size(crate::theme::text_px(10.))
                                .text_color(p.danger())
                                .child(self.error.clone()),
                        )
                    })
                    .child(
                        item(2, "打开面板")
                            .h(px(28.))
                            .rounded(px(12.))
                            .hover(|s| s.bg(p.hover()))
                            .child("打开面板"),
                    )
                    .child(
                        item(3, "退出")
                            .h(px(28.))
                            .rounded(px(12.))
                            .text_color(p.danger())
                            .hover(|s| s.bg(p.danger_soft()))
                            .child("退出"),
                    ),
            )
    }
}
