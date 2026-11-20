use gpui::{App, Bounds, Pixels, WindowBounds, point, px, size};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, GetLastError, WAIT_OBJECT_0},
    System::Threading::{CreateEventW, SetEvent, WaitForSingleObject},
};

pub struct Instance(OwnedHandle);
impl Instance {
    pub fn requested(&self) -> bool {
        unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) == WAIT_OBJECT_0 }
    }
}
pub fn instance(hidden: bool) -> Result<Option<Instance>, String> {
    let sid = singboard_service::ipc::current_user_sid().map_err(|e| e.to_string())?;
    let name = format!("Local\\Singboard.Native.Show.{sid}\0")
        .encode_utf16()
        .collect::<Vec<_>>();
    let raw = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
    if raw.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    if exists {
        if !hidden {
            unsafe {
                SetEvent(raw);
            }
        }
        return Ok(None);
    }
    Ok(Some(Instance(handle)))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Placement {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    maximized: bool,
}

pub fn save(window: WindowBounds) -> Result<(), String> {
    let bounds = window.get_bounds();
    let p = Placement {
        x: bounds.origin.x.into(),
        y: bounds.origin.y.into(),
        w: bounds.size.width.into(),
        h: bounds.size.height.into(),
        maximized: matches!(window, WindowBounds::Maximized(_)),
    };
    if p.w < 800. || p.h < 500. || p.x < -30000. || p.y < -30000. {
        return Ok(());
    }
    let dir = crate::service::data_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    singboard_stream::profiles::write_atomic(
        &dir.join("window.json"),
        &serde_json::to_vec(&p).map_err(|e| e.to_string())?,
    )
}
pub fn restore(cx: &App) -> WindowBounds {
    let fallback = WindowBounds::centered(None, size(px(1100.), px(700.)), cx);
    let Some(p) = crate::service::data_dir()
        .ok()
        .and_then(|d| std::fs::read(d.join("window.json")).ok())
        .and_then(|v| serde_json::from_slice::<Placement>(&v).ok())
    else {
        return fallback;
    };
    if p.w < 800. || p.h < 500. || p.w > 32000. || p.h > 32000. {
        return fallback;
    }
    let bounds: Bounds<Pixels> = Bounds {
        origin: point(px(p.x), px(p.y)),
        size: size(px(p.w), px(p.h)),
    };
    if !cx
        .displays()
        .iter()
        .any(|d| d.bounds().contains(&point(px(p.x + 80.), px(p.y + 20.))))
    {
        return fallback;
    }
    if p.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}
