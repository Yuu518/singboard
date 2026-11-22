use gpui::{App, Bounds, Pixels, WindowBounds, point, px, size};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError, LocalFree, WAIT_OBJECT_0},
    Security::{
        Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES,
    },
    System::Threading::{
        CreateEventW, EVENT_MODIFY_STATE, OpenEventW, SetEvent, WaitForSingleObject,
    },
    UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow},
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
    let sddl = singboard_service::ipc::wide(&format!(
        "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{sid})"
    ));
    let mut descriptor = std::ptr::null_mut();
    let has_descriptor = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) != 0
    };
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let raw = unsafe {
        CreateEventW(
            if has_descriptor { &attrs } else { std::ptr::null() },
            0,
            0,
            name.as_ptr(),
        )
    };
    let last_err = unsafe { GetLastError() };
    if has_descriptor {
        unsafe {
            LocalFree(descriptor);
        }
    }
    if raw.is_null() {
        if last_err == ERROR_ACCESS_DENIED {
            let opened = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
            if !opened.is_null() {
                let _handle = unsafe { OwnedHandle::from_raw_handle(opened) };
                if !hidden {
                    unsafe {
                        AllowSetForegroundWindow(ASFW_ANY);
                        SetEvent(opened);
                    }
                }
                return Ok(None);
            }
        }
        return Err(std::io::Error::from_raw_os_error(last_err as i32).to_string());
    }
    let exists = last_err == ERROR_ALREADY_EXISTS;
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    if exists {
        if !hidden {
            unsafe {
                AllowSetForegroundWindow(ASFW_ANY);
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
    if !p.x.is_finite()
        || !p.y.is_finite()
        || !p.w.is_finite()
        || !p.h.is_finite()
        || p.w < 800.
        || p.h < 500.
        || p.w > 32000.
        || p.h > 32000.
        || p.x < -30000.
        || p.y < -30000.
        || p.x > 30000.
        || p.y > 30000.
    {
        return Ok(());
    }
    let dir = crate::service::data_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    singboard_core::profiles::write_atomic(
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
    if !p.x.is_finite()
        || !p.y.is_finite()
        || !p.w.is_finite()
        || !p.h.is_finite()
        || p.w < 800.
        || p.h < 500.
        || p.w > 32000.
        || p.h > 32000.
    {
        return fallback;
    }
    let bounds: Bounds<Pixels> = Bounds {
        origin: point(px(p.x), px(p.y)),
        size: size(px(p.w), px(p.h)),
    };
    let probes = [
        point(px(p.x + 240.), px(p.y + 20.)),
        point(px(p.x + p.w - 60.), px(p.y + 20.)),
    ];
    let visible = cx.displays().iter().any(|d| {
        let db = d.bounds();
        let safe_bottom = db.origin.y + db.size.height - px(48.);
        probes
            .iter()
            .all(|pt| db.contains(pt) && pt.y < safe_bottom)
    });
    if !visible {
        return fallback;
    }
    if p.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}
