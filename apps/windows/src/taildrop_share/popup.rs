use super::{EndpointPeers, LoadResult, PeerItem, ServiceCandidate, format_bytes};
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr;
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
    CreatePen, CreateRoundRectRgn, CreateSolidBrush, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, Ellipse, EndPaint, FW_MEDIUM,
    FW_NORMAL, FW_SEMIBOLD, FillRect, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromPoint, PAINTSTRUCT, PS_SOLID, RoundRect, SRCCOPY, SelectObject,
    SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_TIP, NIIF_ERROR, NIIF_NONE, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW,
    HWND_TOPMOST, IDC_ARROW, IDC_HAND, IMAGE_ICON, KillTimer, LR_DEFAULTSIZE, LWA_ALPHA,
    LoadCursorW, LoadImageW, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, SW_HIDE,
    SW_SHOW, SWP_NOACTIVATE, SetCursor, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WA_INACTIVE, WM_ACTIVATE,
    WM_APP, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCCREATE, WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

const WM_TARGETS_LOADED: u32 = WM_APP + 1;
const TIMER_REVEAL_FALLBACK: usize = 1;
const TIMER_FADE_IN: usize = 2;
const TARGET_ALPHA: u8 = 244;
const MAX_VISIBLE_ROWS: usize = 6;
const POPUP_WIDTH_DP: i32 = 312;
const HEADER_HEIGHT_DP: i32 = 56;
const CHIP_BAR_HEIGHT_DP: i32 = 36;
const ROW_HEIGHT_DP: i32 = 50;
const STATUS_HEIGHT_DP: i32 = 58;
const PAD_DP: i32 = 10;

pub struct SelectedTarget {
    pub service_tag: Option<String>,
    pub endpoint_tag: String,
    pub peer: PeerItem,
}

#[derive(Clone, Copy)]
struct Palette {
    bg: COLORREF,
    border: COLORREF,
    divider: COLORREF,
    ink: COLORREF,
    muted: COLORREF,
    hover_bg: COLORREF,
    hover_border: COLORREF,
    pill_bg: COLORREF,
    pill_sel_bg: COLORREF,
    pill_sel_border: COLORREF,
    online: COLORREF,
    offline: COLORREF,
    error: COLORREF,
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: rgb(30, 32, 40),
            border: rgb(72, 76, 92),
            divider: rgb(52, 55, 68),
            ink: rgb(242, 242, 247),
            muted: rgb(168, 170, 184),
            hover_bg: rgb(48, 52, 66),
            hover_border: rgb(88, 96, 122),
            pill_bg: rgb(44, 47, 58),
            pill_sel_bg: rgb(62, 70, 94),
            pill_sel_border: rgb(120, 134, 176),
            online: rgb(48, 209, 88),
            offline: rgb(128, 130, 142),
            error: rgb(255, 130, 122),
        }
    } else {
        Palette {
            bg: rgb(250, 249, 252),
            border: rgb(216, 218, 226),
            divider: rgb(232, 233, 240),
            ink: rgb(26, 28, 34),
            muted: rgb(100, 102, 114),
            hover_bg: rgb(235, 237, 244),
            hover_border: rgb(200, 204, 218),
            pill_bg: rgb(238, 240, 246),
            pill_sel_bg: rgb(224, 228, 240),
            pill_sel_border: rgb(166, 176, 204),
            online: rgb(52, 199, 89),
            offline: rgb(156, 158, 168),
            error: rgb(184, 32, 24),
        }
    }
}

fn detect_dark_mode() -> bool {
    let appearance = crate::theme::Appearance::load();
    match appearance.mode {
        crate::theme::Mode::Dark => true,
        crate::theme::Mode::Light => false,
        _ => RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
            .map(|v| v == 0)
            .unwrap_or(false),
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

struct PopupState {
    hwnd: HWND,
    dpi: u32,
    anchor: POINT,
    work_area: RECT,
    win11_round: bool,
    pal: Palette,
    subtitle: String,
    data_dir: PathBuf,
    loading: bool,
    error: String,
    services: Vec<ServiceCandidate>,
    selected_service: Option<String>,
    endpoints: Vec<EndpointPeers>,
    selected_endpoint: usize,
    hovered_row: Option<usize>,
    selected_row: usize,
    scroll_offset: usize,
    esc_rect: RECT,
    chip_rects: Vec<(RECT, usize, bool)>,
    pending_load: Arc<Mutex<Option<LoadResult>>>,
    chosen: Option<SelectedTarget>,
    shown: bool,
    fade_step: u8,
}

impl PopupState {
    fn px(&self, dp: i32) -> i32 {
        (dp * self.dpi as i32 + 48) / 96
    }

    fn current_endpoint(&self) -> Option<&EndpointPeers> {
        self.endpoints.get(self.selected_endpoint).or_else(|| self.endpoints.first())
    }

    fn peers(&self) -> &[PeerItem] {
        self.current_endpoint().map_or(&[], |e| e.peers.as_slice())
    }

    fn has_chips(&self) -> bool {
        self.services.len() > 1 || self.endpoints.len() > 1
    }

    fn desired_height_px(&self) -> i32 {
        let mut dp = HEADER_HEIGHT_DP + PAD_DP;
        if self.has_chips() {
            dp += CHIP_BAR_HEIGHT_DP;
        }
        if self.loading || !self.error.is_empty() || self.peers().is_empty() {
            dp += STATUS_HEIGHT_DP;
        } else {
            let rows = self.peers().len().min(MAX_VISIBLE_ROWS) as i32;
            dp += rows * ROW_HEIGHT_DP + PAD_DP;
        }
        self.px(dp)
    }

    fn desired_width_px(&self) -> i32 {
        self.px(POPUP_WIDTH_DP)
    }

    fn list_top_px(&self) -> i32 {
        let mut dp = HEADER_HEIGHT_DP + 6;
        if self.has_chips() {
            dp += CHIP_BAR_HEIGHT_DP;
        }
        self.px(dp)
    }

    fn place_window(&mut self) {
        let w = self.desired_width_px();
        let h = self.desired_height_px();
        let area = self.work_area;
        let x = if self.anchor.x + w <= area.right {
            self.anchor.x
        } else {
            (self.anchor.x - w).max(area.left)
        };
        let y = if self.anchor.y + h <= area.bottom {
            self.anchor.y
        } else {
            (self.anchor.y - h).max(area.top)
        };
        let left = x.clamp(area.left, (area.right - w).max(area.left));
        let top = y.clamp(area.top, (area.bottom - h).max(area.top));
        unsafe {
            SetWindowPos(self.hwnd, HWND_TOPMOST, left, top, w, h, SWP_NOACTIVATE);
            if !self.win11_round {
                let r = self.px(14);
                let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, r, r);
                if !rgn.is_null() {
                    SetWindowRgn(self.hwnd, rgn, 1);
                }
            }
        }
    }

    fn start_load(&mut self, service_tag: Option<String>) {
        self.loading = true;
        self.error.clear();
        if service_tag.is_some() {
            self.selected_service = service_tag.clone();
        }
        let slot = self.pending_load.clone();
        let dir = self.data_dir.clone();
        let hwnd = self.hwnd as usize;
        std::thread::spawn(move || {
            let loaded = super::load_tailscale_targets(dir, service_tag);
            if let Ok(mut guard) = slot.lock() {
                *guard = Some(loaded);
            }
            unsafe {
                PostMessageW(hwnd as HWND, WM_TARGETS_LOADED, 0, 0);
            }
        });
    }

    fn reveal_if_hidden(&mut self) {
        if self.shown {
            return;
        }
        self.shown = true;
        self.fade_step = 0;
        unsafe {
            KillTimer(self.hwnd, TIMER_REVEAL_FALLBACK);
        }
        self.place_window();
        unsafe {
            SetLayeredWindowAttributes(self.hwnd, 0, 0, LWA_ALPHA);
            ShowWindow(self.hwnd, SW_SHOW);
            UpdateWindow(self.hwnd);
            SetForegroundWindow(self.hwnd);
            SetTimer(self.hwnd, TIMER_FADE_IN, 14, None);
        }
    }

    fn step_fade_in(&mut self) {
        self.fade_step = self.fade_step.saturating_add(1);
        const TOTAL_STEPS: u8 = 6;
        let t = (self.fade_step as f32 / TOTAL_STEPS as f32).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        let alpha = (TARGET_ALPHA as f32 * eased).round() as u8;
        unsafe {
            SetLayeredWindowAttributes(self.hwnd, 0, alpha, LWA_ALPHA);
        }
        if self.fade_step >= TOTAL_STEPS {
            unsafe {
                KillTimer(self.hwnd, TIMER_FADE_IN);
            }
        }
    }

    fn apply_loaded(&mut self) {
        let Some(loaded) = self.pending_load.lock().ok().and_then(|mut g| g.take()) else {
            return;
        };
        self.loading = false;
        self.error = loaded.error;
        self.services = loaded.services;
        self.selected_service = loaded.selected_service;
        self.endpoints = loaded.endpoints;
        self.selected_endpoint = 0;
        self.scroll_offset = 0;
        self.selected_row = 0;
        self.hovered_row = None;
        if !self.shown {
            self.reveal_if_hidden();
        } else {
            self.place_window();
            unsafe {
                InvalidateRect(self.hwnd, ptr::null(), 0);
            }
        }
    }

    fn row_at_point(&self, x: i32, y: i32) -> Option<usize> {
        if self.loading || !self.error.is_empty() || self.peers().is_empty() {
            return None;
        }
        let pad = self.px(PAD_DP);
        let w = self.desired_width_px();
        if x < pad || x > w - pad {
            return None;
        }
        let top = self.list_top_px();
        if y < top {
            return None;
        }
        let row_h = self.px(ROW_HEIGHT_DP);
        let vis = ((y - top) / row_h) as usize;
        if vis >= MAX_VISIBLE_ROWS {
            return None;
        }
        let idx = self.scroll_offset + vis;
        if idx < self.peers().len() {
            Some(idx)
        } else {
            None
        }
    }

    fn pick_row(&mut self, index: usize) {
        let Some(ep) = self.current_endpoint() else {
            return;
        };
        let Some(peer) = ep.peers.get(index).cloned() else {
            return;
        };
        self.chosen = Some(SelectedTarget {
            service_tag: self.selected_service.clone(),
            endpoint_tag: ep.endpoint_tag.clone(),
            peer,
        });
        unsafe {
            ShowWindow(self.hwnd, SW_HIDE);
            DestroyWindow(self.hwnd);
        }
    }
}

fn pt_in_rect(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
}

unsafe fn make_font(height: i32, weight: i32, family: &str) -> *mut c_void {
    let face = wide(family);
    let mut name = [0u16; 32];
    let n = face.len().min(32);
    name[..n].copy_from_slice(&face[..n]);
    unsafe {
        CreateFontW(
            -height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            1,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            name.as_ptr(),
        )
    }
}

unsafe fn draw_round_box(
    hdc: *mut c_void,
    r: RECT,
    radius: i32,
    fill: COLORREF,
    stroke: Option<COLORREF>,
) {
    let brush = unsafe { CreateSolidBrush(fill) };
    let pen = unsafe { CreatePen(PS_SOLID, 1, stroke.unwrap_or(fill)) };
    let old_brush = unsafe { SelectObject(hdc, brush) };
    let old_pen = unsafe { SelectObject(hdc, pen) };
    unsafe {
        RoundRect(hdc, r.left, r.top, r.right, r.bottom, radius, radius);
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        DeleteObject(brush);
        DeleteObject(pen);
    }
}

unsafe fn draw_text_line(
    hdc: *mut c_void,
    font: *mut c_void,
    color: COLORREF,
    text: &str,
    mut rect: RECT,
) {
    let mut buf = wide(text);
    let old_font = unsafe { SelectObject(hdc, font) };
    unsafe {
        SetTextColor(hdc, color);
        DrawTextW(
            hdc,
            buf.as_mut_ptr(),
            (buf.len().saturating_sub(1)) as i32,
            &mut rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(hdc, old_font);
    }
}

unsafe fn paint_popup(state: &mut PopupState) {
    let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
    let screen_dc = unsafe { BeginPaint(state.hwnd, &mut ps) };
    if screen_dc.is_null() {
        return;
    }
    let w = state.desired_width_px();
    let h = state.desired_height_px();
    let mem_dc = unsafe { CreateCompatibleDC(screen_dc) };
    let bmp = unsafe { CreateCompatibleBitmap(screen_dc, w, h) };
    let old_bmp = unsafe { SelectObject(mem_dc, bmp) };
    unsafe { SetBkMode(mem_dc, TRANSPARENT as i32) };

    let bg_brush = unsafe { CreateSolidBrush(state.pal.bg) };
    let full = RECT {
        left: 0,
        top: 0,
        right: w,
        bottom: h,
    };
    unsafe {
        FillRect(mem_dc, &full, bg_brush);
        DeleteObject(bg_brush);
    }
    if !state.win11_round {
        unsafe {
            draw_round_box(
                mem_dc,
                RECT {
                    left: 0,
                    top: 0,
                    right: w - 1,
                    bottom: h - 1,
                },
                state.px(14),
                state.pal.bg,
                Some(state.pal.border),
            );
        }
    }

    let font_title = unsafe { make_font(state.px(13), FW_SEMIBOLD as i32, "Microsoft YaHei UI") };
    let font_sub = unsafe { make_font(state.px(11), FW_NORMAL as i32, "Microsoft YaHei UI") };
    let font_row = unsafe { make_font(state.px(13), FW_MEDIUM as i32, "Microsoft YaHei UI") };
    let font_mono = unsafe { make_font(state.px(11), FW_NORMAL as i32, "Consolas") };
    let font_badge = unsafe { make_font(state.px(10), FW_MEDIUM as i32, "Microsoft YaHei UI") };

    let pad = state.px(14);
    let esc_w = state.px(36);
    let esc_h = state.px(22);
    state.esc_rect = RECT {
        left: w - pad - esc_w,
        top: state.px(12),
        right: w - pad,
        bottom: state.px(12) + esc_h,
    };
    unsafe {
        draw_round_box(
            mem_dc,
            state.esc_rect,
            esc_h,
            state.pal.pill_bg,
            Some(state.pal.divider),
        );
        let mut esc_text_rect = state.esc_rect;
        esc_text_rect.left += state.px(8);
        draw_text_line(mem_dc, font_badge, state.pal.muted, "Esc", esc_text_rect);
    }

    unsafe {
        draw_text_line(
            mem_dc,
            font_title,
            state.pal.ink,
            "通过 Taildrop 发送",
            RECT {
                left: pad,
                top: state.px(10),
                right: state.esc_rect.left - state.px(8),
                bottom: state.px(30),
            },
        );
        draw_text_line(
            mem_dc,
            font_sub,
            state.pal.muted,
            &state.subtitle,
            RECT {
                left: pad,
                top: state.px(30),
                right: w - pad,
                bottom: state.px(48),
            },
        );
    }

    let div_y = state.px(HEADER_HEIGHT_DP);
    let div_brush = unsafe { CreateSolidBrush(state.pal.divider) };
    unsafe {
        FillRect(
            mem_dc,
            &RECT {
                left: state.px(PAD_DP),
                top: div_y,
                right: w - state.px(PAD_DP),
                bottom: div_y + 1,
            },
            div_brush,
        );
        DeleteObject(div_brush);
    }

    state.chip_rects.clear();
    if state.has_chips() {
        let mut cx = state.px(PAD_DP);
        let cy = div_y + state.px(6);
        let ch = state.px(24);
        if state.services.len() > 1 {
            for (idx, svc) in state.services.iter().enumerate() {
                let label = if svc.service_tag.is_empty() {
                    svc.url.as_str()
                } else {
                    svc.service_tag.as_str()
                };
                let cw = state.px((label.chars().count() as i32 * 8 + 20).clamp(52, 120));
                if cx + cw > w - state.px(PAD_DP) {
                    break;
                }
                let r = RECT {
                    left: cx,
                    top: cy,
                    right: cx + cw,
                    bottom: cy + ch,
                };
                let sel = state.selected_service.as_deref() == Some(svc.service_tag.as_str());
                unsafe {
                    draw_round_box(
                        mem_dc,
                        r,
                        ch,
                        if sel { state.pal.pill_sel_bg } else { state.pal.pill_bg },
                        Some(if sel { state.pal.pill_sel_border } else { state.pal.divider }),
                    );
                    draw_text_line(
                        mem_dc,
                        font_badge,
                        state.pal.ink,
                        label,
                        RECT {
                            left: r.left + state.px(8),
                            top: r.top,
                            right: r.right - state.px(6),
                            bottom: r.bottom,
                        },
                    );
                }
                state.chip_rects.push((r, idx, true));
                cx += cw + state.px(6);
            }
        } else if state.endpoints.len() > 1 {
            for (idx, ep) in state.endpoints.iter().enumerate() {
                let cw = state.px((ep.endpoint_tag.chars().count() as i32 * 8 + 20).clamp(52, 120));
                if cx + cw > w - state.px(PAD_DP) {
                    break;
                }
                let r = RECT {
                    left: cx,
                    top: cy,
                    right: cx + cw,
                    bottom: cy + ch,
                };
                let sel = idx == state.selected_endpoint;
                unsafe {
                    draw_round_box(
                        mem_dc,
                        r,
                        ch,
                        if sel { state.pal.pill_sel_bg } else { state.pal.pill_bg },
                        Some(if sel { state.pal.pill_sel_border } else { state.pal.divider }),
                    );
                    draw_text_line(
                        mem_dc,
                        font_badge,
                        state.pal.ink,
                        &ep.endpoint_tag,
                        RECT {
                            left: r.left + state.px(8),
                            top: r.top,
                            right: r.right - state.px(6),
                            bottom: r.bottom,
                        },
                    );
                }
                state.chip_rects.push((r, idx, false));
                cx += cw + state.px(6);
            }
        }
    }

    let list_top = state.list_top_px();
    if state.loading {
        unsafe {
            draw_text_line(
                mem_dc,
                font_sub,
                state.pal.muted,
                "正在获取 Tailscale 接收设备…",
                RECT {
                    left: pad,
                    top: list_top,
                    right: w - pad,
                    bottom: list_top + state.px(STATUS_HEIGHT_DP - 12),
                },
            );
        }
    } else if !state.error.is_empty() {
        unsafe {
            draw_text_line(
                mem_dc,
                font_sub,
                state.pal.error,
                &state.error,
                RECT {
                    left: pad,
                    top: list_top,
                    right: w - pad,
                    bottom: list_top + state.px(STATUS_HEIGHT_DP - 12),
                },
            );
        }
    } else if state.peers().is_empty() {
        let msg = if state.services.len() > 1 && state.selected_service.is_none() {
            "请先在上方选择 Service API 服务"
        } else {
            "当前 Tailnet 中没有可接收文件的设备"
        };
        unsafe {
            draw_text_line(
                mem_dc,
                font_sub,
                state.pal.muted,
                msg,
                RECT {
                    left: pad,
                    top: list_top,
                    right: w - pad,
                    bottom: list_top + state.px(STATUS_HEIGHT_DP - 12),
                },
            );
        }
    } else {
        let row_h = state.px(ROW_HEIGHT_DP);
        let row_pad = state.px(PAD_DP);
        let peers_snapshot: Vec<PeerItem> = state.peers().to_vec();
        let active_idx = state.hovered_row.unwrap_or(state.selected_row);
        for vis in 0..MAX_VISIBLE_ROWS {
            let idx = state.scroll_offset + vis;
            let Some(peer) = peers_snapshot.get(idx) else {
                break;
            };
            let top = list_top + vis as i32 * row_h;
            let rect = RECT {
                left: row_pad,
                top: top + state.px(2),
                right: w - row_pad,
                bottom: top + row_h - state.px(2),
            };
            if idx == active_idx {
                unsafe {
                    draw_round_box(
                        mem_dc,
                        rect,
                        state.px(10),
                        state.pal.hover_bg,
                        Some(state.pal.hover_border),
                    );
                }
            }
            let dot_size = state.px(8);
            let dot_x = rect.left + state.px(11);
            let dot_y = (rect.top + rect.bottom - dot_size) / 2;
            let dot_color = if peer.online {
                state.pal.online
            } else {
                state.pal.offline
            };
            unsafe {
                let b = CreateSolidBrush(dot_color);
                let p = CreatePen(PS_SOLID, 1, dot_color);
                let ob = SelectObject(mem_dc, b);
                let op = SelectObject(mem_dc, p);
                Ellipse(mem_dc, dot_x, dot_y, dot_x + dot_size, dot_y + dot_size);
                SelectObject(mem_dc, ob);
                SelectObject(mem_dc, op);
                DeleteObject(b);
                DeleteObject(p);
            }

            let text_left = dot_x + dot_size + state.px(10);
            let mut name_right = rect.right - state.px(10);
            if peer.is_last {
                let badge_w = state.px(34);
                let badge_h = state.px(18);
                let badge_rect = RECT {
                    left: rect.right - state.px(10) - badge_w,
                    top: rect.top + state.px(6),
                    right: rect.right - state.px(10),
                    bottom: rect.top + state.px(6) + badge_h,
                };
                name_right = badge_rect.left - state.px(6);
                unsafe {
                    draw_round_box(mem_dc, badge_rect, badge_h, state.pal.pill_bg, None);
                    draw_text_line(
                        mem_dc,
                        font_badge,
                        state.pal.muted,
                        "上次",
                        RECT {
                            left: badge_rect.left + state.px(6),
                            top: badge_rect.top,
                            right: badge_rect.right,
                            bottom: badge_rect.bottom,
                        },
                    );
                }
            }

            let meta = match (peer.os.is_empty(), peer.primary_ip.is_empty()) {
                (false, false) => format!("{} · {}", peer.os, peer.primary_ip),
                (false, true) => peer.os.clone(),
                (true, false) => peer.primary_ip.clone(),
                (true, true) => String::new(),
            };
            unsafe {
                draw_text_line(
                    mem_dc,
                    font_row,
                    state.pal.ink,
                    &peer.display_name,
                    RECT {
                        left: text_left,
                        top: rect.top + state.px(5),
                        right: name_right,
                        bottom: rect.top + state.px(25),
                    },
                );
                if !meta.is_empty() {
                    draw_text_line(
                        mem_dc,
                        font_mono,
                        state.pal.muted,
                        &meta,
                        RECT {
                            left: text_left,
                            top: rect.top + state.px(24),
                            right: rect.right - state.px(10),
                            bottom: rect.bottom - state.px(4),
                        },
                    );
                }
            }
        }
    }

    unsafe {
        DeleteObject(font_title);
        DeleteObject(font_sub);
        DeleteObject(font_row);
        DeleteObject(font_mono);
        DeleteObject(font_badge);
        BitBlt(screen_dc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
        SelectObject(mem_dc, old_bmp);
        DeleteObject(bmp);
        DeleteDC(mem_dc);
        EndPaint(state.hwnd, &ps);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
        if !cs.is_null() {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
            }
        }
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PopupState;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let state = unsafe { &mut *ptr };
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            unsafe { paint_popup(state) };
            0
        }
        WM_TARGETS_LOADED => {
            state.apply_loaded();
            0
        }
        WM_TIMER => {
            if wparam == TIMER_REVEAL_FALLBACK {
                state.reveal_if_hidden();
            } else if wparam == TIMER_FADE_IN {
                state.step_fade_in();
            }
            0
        }
        WM_ACTIVATE => {
            if state.shown && (wparam & 0xFFFF) as u32 == WA_INACTIVE {
                unsafe { DestroyWindow(hwnd) };
            }
            0
        }
        WM_SETCURSOR => {
            let mut pt: POINT = unsafe { std::mem::zeroed() };
            if unsafe { GetCursorPos(&mut pt) } != 0 {
                let mut rc: RECT = unsafe { std::mem::zeroed() };
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rc);
                }
                let lx = pt.x - rc.left;
                let ly = pt.y - rc.top;
                let clickable = state.row_at_point(lx, ly).is_some()
                    || pt_in_rect(&state.esc_rect, lx, ly)
                    || state.chip_rects.iter().any(|(r, _, _)| pt_in_rect(r, lx, ly));
                let cur = unsafe { LoadCursorW(ptr::null_mut(), if clickable { IDC_HAND } else { IDC_ARROW }) };
                unsafe { SetCursor(cur) };
                return 1;
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_MOUSEMOVE => {
            let x = (lparam & 0xFFFF) as i16 as i32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
            let next = state.row_at_point(x, y);
            if next != state.hovered_row {
                state.hovered_row = next;
                if let Some(idx) = next {
                    state.selected_row = idx;
                }
                unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
            }
            0
        }
        WM_LBUTTONUP => {
            let x = (lparam & 0xFFFF) as i16 as i32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
            if pt_in_rect(&state.esc_rect, x, y) {
                unsafe { DestroyWindow(hwnd) };
                return 0;
            }
            if let Some((_, idx, is_service)) =
                state.chip_rects.iter().find(|(r, _, _)| pt_in_rect(r, x, y)).copied()
            {
                if is_service {
                    if let Some(svc) = state.services.get(idx) {
                        let tag = svc.service_tag.clone();
                        state.start_load(Some(tag));
                        unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
                    }
                } else if idx < state.endpoints.len() {
                    state.selected_endpoint = idx;
                    state.scroll_offset = 0;
                    state.selected_row = 0;
                    state.hovered_row = None;
                    state.place_window();
                    unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
                }
                return 0;
            }
            if let Some(idx) = state.row_at_point(x, y) {
                state.pick_row(idx);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) & 0xFFFF) as i16;
            let total = state.peers().len();
            if total > MAX_VISIBLE_ROWS {
                let max_off = total - MAX_VISIBLE_ROWS;
                if delta < 0 && state.scroll_offset < max_off {
                    state.scroll_offset += 1;
                    unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
                } else if delta > 0 && state.scroll_offset > 0 {
                    state.scroll_offset -= 1;
                    unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
                }
            }
            0
        }
        WM_KEYDOWN => {
            match wparam as u16 {
                VK_ESCAPE => unsafe {
                    DestroyWindow(hwnd);
                },
                VK_UP => {
                    let count = state.peers().len();
                    if count > 0 {
                        state.hovered_row = None;
                        state.selected_row = state.selected_row.saturating_sub(1);
                        if state.selected_row < state.scroll_offset {
                            state.scroll_offset = state.selected_row;
                        }
                        unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
                    }
                }
                VK_DOWN => {
                    let count = state.peers().len();
                    if count > 0 && state.selected_row + 1 < count {
                        state.hovered_row = None;
                        state.selected_row += 1;
                        if state.selected_row >= state.scroll_offset + MAX_VISIBLE_ROWS {
                            state.scroll_offset = state.selected_row + 1 - MAX_VISIBLE_ROWS;
                        }
                        unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
                    }
                }
                VK_RETURN => {
                    if !state.peers().is_empty() {
                        let idx = state.hovered_row.unwrap_or(state.selected_row);
                        state.pick_row(idx);
                    }
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                KillTimer(hwnd, TIMER_REVEAL_FALLBACK);
                KillTimer(hwnd, TIMER_FADE_IN);
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn pick_target(
    data_dir: PathBuf,
    paths: &[PathBuf],
    anchor: Option<POINT>,
) -> Result<Option<SelectedTarget>, String> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let anchor = anchor.unwrap_or_else(|| {
        let mut pt: POINT = unsafe { std::mem::zeroed() };
        unsafe { GetCursorPos(&mut pt) };
        pt
    });
    let monitor = unsafe { MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    let work_area = if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
        info.rcWork
    } else {
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        }
    };
    let mut dx = 96u32;
    let mut dy = 96u32;
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } < 0 {
        dx = 96;
    }

    let total_bytes: u64 = paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
        .sum();
    let subtitle = if paths.len() == 1 {
        let name = paths[0]
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("1 个文件");
        format!("{name} · {}", format_bytes(total_bytes))
    } else {
        format!("{} 个文件 · {}", paths.len(), format_bytes(total_bytes))
    };

    let dark = detect_dark_mode();
    let class_name = wide("SingboardTaildropPopup");
    let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: ptr::null_mut(),
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        hbrBackground: ptr::null_mut(),
        lpszMenuName: ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: ptr::null_mut(),
    };
    unsafe { RegisterClassExW(&wc) };

    let mut state = Box::new(PopupState {
        hwnd: ptr::null_mut(),
        dpi: dx,
        anchor,
        work_area,
        win11_round: false,
        pal: palette(dark),
        subtitle,
        data_dir,
        loading: true,
        error: String::new(),
        services: Vec::new(),
        selected_service: None,
        endpoints: Vec::new(),
        selected_endpoint: 0,
        hovered_row: None,
        selected_row: 0,
        scroll_offset: 0,
        esc_rect: unsafe { std::mem::zeroed() },
        chip_rects: Vec::new(),
        pending_load: Arc::new(Mutex::new(None)),
        chosen: None,
        shown: false,
        fade_step: 0,
    });

    let title = wide("通过 Taildrop 发送");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            state.desired_width_px(),
            state.desired_height_px(),
            ptr::null_mut(),
            ptr::null_mut(),
            hinstance,
            (&mut *state as *mut PopupState) as *const c_void,
        )
    };
    if hwnd.is_null() {
        return Err("无法创建 Taildrop 发送浮窗".into());
    }
    state.hwnd = hwnd;

    unsafe {
        SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA);
        let dark_val: i32 = if dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark_val as *const i32 as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
        let corner = DWMWCP_ROUND;
        state.win11_round = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corner as *const i32 as *const c_void,
            std::mem::size_of::<i32>() as u32,
        ) == 0;
    }

    state.place_window();
    state.start_load(None);
    unsafe {
        SetTimer(hwnd, TIMER_REVEAL_FALLBACK, 240, None);
    }

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(state.chosen.take())
}

pub struct BalloonNotifier {
    hwnd: HWND,
    icon: *mut c_void,
    active_uid: Option<u32>,
    next_uid: u32,
}

fn copy_wide_fixed<const N: usize>(dst: &mut [u16; N], text: &str) {
    dst.fill(0);
    for (slot, unit) in dst.iter_mut().take(N.saturating_sub(1)).zip(text.encode_utf16()) {
        *slot = unit;
    }
}

impl BalloonNotifier {
    pub fn new() -> Self {
        let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
        let class_name = wide("SingboardTaildropNotify");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: ptr::null_mut(),
            hCursor: ptr::null_mut(),
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: ptr::null_mut(),
        };
        unsafe { RegisterClassExW(&wc) };
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                class_name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                hinstance,
                ptr::null(),
            )
        };
        let icon = unsafe {
            LoadImageW(
                hinstance,
                1usize as *const u16,
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTSIZE,
            )
        };
        Self {
            hwnd,
            icon,
            active_uid: None,
            next_uid: 1,
        }
    }

    fn delete_active(&mut self) {
        if let Some(uid) = self.active_uid.take()
            && !self.hwnd.is_null()
        {
            let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = self.hwnd;
            nid.uID = uid;
            unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
        }
    }

    pub fn notify(&mut self, title: &str, body: &str, error: bool) {
        if self.hwnd.is_null() {
            return;
        }
        self.delete_active();
        let uid = self.next_uid;
        self.next_uid = self.next_uid.wrapping_add(1).max(1);
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = uid;
        nid.uFlags = NIF_TIP | NIF_INFO | if self.icon.is_null() { 0 } else { NIF_ICON };
        nid.hIcon = self.icon;
        nid.dwInfoFlags = if error { NIIF_ERROR } else { NIIF_NONE };
        copy_wide_fixed(&mut nid.szTip, "Singboard Taildrop");
        copy_wide_fixed(&mut nid.szInfoTitle, title);
        copy_wide_fixed(&mut nid.szInfo, body);
        if unsafe { Shell_NotifyIconW(NIM_ADD, &nid) } != 0 {
            self.active_uid = Some(uid);
        }
    }

    pub fn flush_and_close(mut self, dwell_ms: u32) {
        if self.active_uid.is_some() && dwell_ms > 0 {
            let timer = unsafe { SetTimer(self.hwnd, 1, dwell_ms, None) };
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
                if msg.message == WM_TIMER && msg.hwnd == self.hwnd && msg.wParam == timer {
                    break;
                }
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if timer != 0 {
                unsafe { KillTimer(self.hwnd, timer) };
            }
        }
        self.cleanup();
    }

    fn cleanup(&mut self) {
        self.delete_active();
        if !self.hwnd.is_null() {
            unsafe { DestroyWindow(self.hwnd) };
            self.hwnd = ptr::null_mut();
        }
        if !self.icon.is_null() {
            unsafe { DestroyIcon(self.icon) };
            self.icon = ptr::null_mut();
        }
    }
}

impl Drop for BalloonNotifier {
    fn drop(&mut self) {
        self.cleanup();
    }
}
