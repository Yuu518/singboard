use gpui::{App, Global, Rgba, Window, WindowAppearance, rgb, rgba};
use gpui_component::{Theme, ThemeMode};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SystemParametersInfoW,
};

pub fn system_ui_font() -> &'static str {
    static FONT: OnceLock<String> = OnceLock::new();
    FONT.get_or_init(|| {
        let mut metrics: NONCLIENTMETRICSW = unsafe { std::mem::zeroed() };
        metrics.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETNONCLIENTMETRICS,
                metrics.cbSize,
                (&mut metrics as *mut NONCLIENTMETRICSW).cast(),
                0,
            )
        };
        if ok != 0 {
            let face = &metrics.lfMessageFont.lfFaceName;
            let len = face.iter().position(|&c| c == 0).unwrap_or(face.len());
            let name = String::from_utf16_lossy(&face[..len]).trim().to_owned();
            if !name.is_empty() {
                return name;
            }
        }
        "Segoe UI".into()
    })
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Light,
    Dark,
    Auto,
}

pub const MODES: [(Mode, &str); 3] = [
    (Mode::Light, "浅色"),
    (Mode::Dark, "深色"),
    (Mode::Auto, "跟随系统"),
];

#[derive(Default)]
pub struct Appearance {
    pub mode: Mode,
    pub dark: bool,
    wallpaper: bool,
}

impl Global for Appearance {}

fn settings_path() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("Singboard/native/theme.json"))
        .ok_or_else(|| "无法定位本地应用数据目录".into())
}

fn hwnd(window: &Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as _),
        _ => None,
    }
}

pub fn set_caption(window: &Window, dark: bool) {
    if let Some(hwnd) = hwnd(window) {
        let dark = i32::from(dark);
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE as _,
                (&dark as *const i32).cast(),
                4,
            );
        }
    }
}

impl Appearance {
    pub fn load() -> Self {
        let previous = settings_path()
            .ok()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        let mode = crate::maintenance::preferences()
            .ok()
            .and_then(|p| p.theme)
            .and_then(|v| serde_json::from_value(serde_json::Value::String(v)).ok())
            .unwrap_or(previous);
        Self {
            mode,
            ..Default::default()
        }
    }

    pub fn select(
        mode: Mode,
        window: &Window,
        wallpaper: bool,
        cx: &mut App,
    ) -> Result<(), String> {
        singboard_stream::settings::Settings::edit(&crate::service::data_dir()?, |prefs| {
            prefs.theme =
                Some(serde_json::from_value(serde_json::json!(mode)).map_err(|e| e.to_string())?);
            Ok(())
        })?;
        cx.global_mut::<Self>().mode = mode;
        Self::apply(window, wallpaper, cx);
        Ok(())
    }

    pub fn apply(window: &Window, wallpaper: bool, cx: &mut App) {
        let dark = {
            let theme = cx.global_mut::<Self>();
            theme.wallpaper = wallpaper;
            theme.dark = match theme.mode {
                Mode::Light => false,
                Mode::Dark => true,
                Mode::Auto => matches!(
                    window.appearance(),
                    WindowAppearance::Dark | WindowAppearance::VibrantDark
                ),
            };
            theme.dark
        };
        set_caption(window, dark);
        let mode = if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        Theme::change(mode, None, cx);
        Theme::global_mut(cx).font_family = system_ui_font().into();
        cx.refresh_windows();
    }
}

#[derive(Clone, Copy)]
pub struct Palette(pub bool);

impl Palette {
    pub fn get(cx: &App) -> Self {
        Self(cx.global::<Appearance>().dark)
    }
    pub fn choose(self, light: u32, dark: u32) -> u32 {
        if self.0 { dark } else { light }
    }
    pub fn page(self) -> Rgba {
        rgb(self.choose(0xeef2fa, 0x0b0f19))
    }
    pub fn ink(self) -> Rgba {
        rgb(self.choose(0x1c1c1e, 0xf2f2f7))
    }
    pub fn muted(self) -> Rgba {
        rgba(self.choose(0x1c1c1ec4, 0xf2f2f7c8))
    }
    pub fn accent(self) -> Rgba {
        rgb(self.choose(0x0057bd, 0x62afff))
    }
    pub fn primary_fill(self) -> Rgba {
        rgb(self.choose(0x0071eb, 0x0a84ff))
    }
    pub fn primary_soft(self) -> Rgba {
        rgba(self.choose(0x007aff24, 0x0a84ff33))
    }
    pub fn success_dot(self) -> Rgba {
        rgb(self.choose(0x34c759, 0x30d158))
    }
    pub fn success_ink(self) -> Rgba {
        rgb(self.choose(0x1b7a34, 0x30d158))
    }
    pub fn success_soft(self) -> Rgba {
        rgba(self.choose(0x34c75924, 0x30d1582e))
    }
    pub fn warning_dot(self) -> Rgba {
        rgb(self.choose(0xff9500, 0xff9f0a))
    }
    pub fn warning_ink(self) -> Rgba {
        rgb(self.choose(0xa65c00, 0xff9f0a))
    }
    pub fn warning_soft(self) -> Rgba {
        rgba(self.choose(0xff950024, 0xff9f0a2e))
    }
    pub fn danger(self) -> Rgba {
        rgb(self.choose(0xb5150b, 0xff8f87))
    }
    pub fn danger_fill(self) -> Rgba {
        rgb(self.choose(0xd92b20, 0xff453a))
    }
    pub fn danger_soft(self) -> Rgba {
        rgba(self.choose(0xff3b301f, 0xff453a2e))
    }
    pub fn tcp(self) -> Rgba {
        rgb(self.choose(0x006a8e, 0x64d2ff))
    }
    pub fn tcp_soft(self) -> Rgba {
        rgba(self.choose(0x007aff1f, 0x64d2ff26))
    }
    pub fn udp(self) -> Rgba {
        rgb(self.choose(0x8e2fbf, 0xd38df5))
    }
    pub fn udp_soft(self) -> Rgba {
        rgba(self.choose(0xaf52de1f, 0xbf5af229))
    }
    pub fn row_error(self) -> Rgba {
        rgba(self.choose(0xff3b3014, 0xff453a1c))
    }
    pub fn row_warn(self) -> Rgba {
        rgba(self.choose(0xff950014, 0xff9f0a1c))
    }
    pub fn hover(self) -> Rgba {
        rgba(self.choose(0x76768012, 0x76768024))
    }
    pub fn selection_border(self) -> Rgba {
        rgba(self.choose(0x007aff66, 0x0a84ff66))
    }
    pub fn chart_ink_rgb(self) -> u32 {
        self.choose(0x1c1c1e, 0xf2f2f7)
    }
    pub fn chart_ink(self, alpha: u8) -> Rgba {
        rgba((self.chart_ink_rgb() << 8) | u32::from(alpha))
    }
    pub fn topology_colors(self) -> [u32; 4] {
        if self.0 {
            [0x5e5ce6, 0x64d2ff, 0xbf5af2, 0xff375f]
        } else {
            [0x5856d6, 0x32ade6, 0xaf52de, 0xff2d55]
        }
    }
    pub fn fill(self) -> Rgba {
        rgba(self.choose(0x7676801f, 0x7676803d))
    }
    pub fn fill_strong(self) -> Rgba {
        rgba(self.choose(0x76768033, 0x76768057))
    }
    pub fn line(self) -> Rgba {
        rgba(self.choose(0x3c3c431f, 0x54545880))
    }
    pub fn rim(self) -> Rgba {
        rgba(self.choose(0x00000018, 0xffffff24))
    }
    pub fn thumb(self) -> Rgba {
        rgb(self.choose(0xffffff, 0x636366))
    }
    pub fn tile(self) -> Rgba {
        rgba(self.choose(0xffffff7a, 0x3a3a406b))
    }
    pub fn toggle_off(self) -> Rgba {
        rgb(self.choose(0xd8d8df, 0x39393d))
    }
    pub fn card(self, translucent: bool) -> Rgba {
        if translucent {
            rgba(self.choose(0xffffffc4, 0x20232dd4))
        } else {
            rgb(self.choose(0xffffff, 0x24262f))
        }
    }
    pub fn card_tinted(self, translucent: bool, tint: f32) -> Rgba {
        if !translucent {
            return self.card(false);
        }
        self.glass(0.30 + 0.32 * tint.clamp(0., 1.))
    }
    pub fn floating(self, tint: f32) -> Rgba {
        self.glass(0.14 + 0.46 * tint.clamp(0., 1.))
    }
    pub fn popover(self, tint: f32) -> Rgba {
        self.glass(0.42 + 0.30 * tint.clamp(0., 1.))
    }
    fn glass(self, alpha: f32) -> Rgba {
        let alpha = (alpha * 255.).round() as u32;
        if self.0 {
            rgba(0x2c2c3000 | alpha)
        } else {
            rgba(0xffffff00 | alpha)
        }
    }
}
