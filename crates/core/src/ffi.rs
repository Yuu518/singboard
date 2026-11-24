use super::{Feed, Subscription};
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

#[cfg(target_os = "android")]
mod android_jni;
mod service_api;

unsafe fn input<'a>(value: *const c_char) -> Option<&'a str> {
    if value.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(value) }.to_str().ok()
}

fn output(value: String) -> *mut c_char {
    let sanitized = if value.as_bytes().contains(&0) {
        value.replace('\0', "")
    } else {
        value
    };
    CString::new(sanitized).map_or(ptr::null_mut(), CString::into_raw)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_clash_request(
    endpoint: *const c_char,
    secret: *const c_char,
    request: *const c_char,
) -> *mut c_char {
    use super::clash::Clash;
    let envelope = match catch_unwind(AssertUnwindSafe(|| {
        let result = (|| {
            let endpoint = unsafe { input(endpoint) }.ok_or("端点地址无效")?;
            let secret = unsafe { input(secret) }.ok_or("密钥无效")?;
            let request = unsafe { input(request) }.ok_or("操作无效")?;
            let command = serde_json::from_str(request).map_err(|_| "操作无效")?;
            super::runtime().block_on(Clash::new(endpoint, secret)?.execute(command))
        })();
        match result {
            Ok(data) => serde_json::json!({"data": data}),
            Err(error) => serde_json::json!({"error": error}),
        }
    })) {
        Ok(envelope) => envelope,
        Err(_) => serde_json::json!({"error": "内部执行异常"}),
    };
    output(envelope.to_string())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_connections_open(
    endpoint: *const c_char,
    secret: *const c_char,
) -> *mut Subscription {
    catch_unwind(AssertUnwindSafe(|| {
        let (Some(endpoint), Some(secret)) = (unsafe { input(endpoint) }, unsafe { input(secret) })
        else {
            return ptr::null_mut();
        };
        Subscription::start(endpoint, secret, Feed::Connections)
            .map_or(ptr::null_mut(), |state| Box::into_raw(Box::new(state)))
    }))
    .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_logs_open(
    endpoint: *const c_char,
    secret: *const c_char,
    level: *const c_char,
) -> *mut Subscription {
    catch_unwind(AssertUnwindSafe(|| {
        let (Some(endpoint), Some(secret)) = (unsafe { input(endpoint) }, unsafe { input(secret) })
        else {
            return ptr::null_mut();
        };
        let level = unsafe { input(level) }.unwrap_or("info");
        Subscription::start_logs(endpoint, secret, level)
            .map_or(ptr::null_mut(), |state| Box::into_raw(Box::new(state)))
    }))
    .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_stream_open(
    endpoint: *const c_char,
    secret: *const c_char,
    traffic: bool,
) -> *mut Subscription {
    catch_unwind(AssertUnwindSafe(|| {
        let (Some(endpoint), Some(secret)) = (unsafe { input(endpoint) }, unsafe { input(secret) })
        else {
            return ptr::null_mut();
        };
        let feed = if traffic { Feed::Traffic } else { Feed::Logs };
        Subscription::start(endpoint, secret, feed)
            .map_or(ptr::null_mut(), |state| Box::into_raw(Box::new(state)))
    }))
    .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_stream_drain(handle: *const Subscription) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(subscription) = (unsafe { handle.as_ref() }) else {
            return ptr::null_mut();
        };
        serde_json::to_string(&subscription.drain()).map_or(ptr::null_mut(), output)
    }))
    .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_stream_close(handle: *mut Subscription) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !handle.is_null() {
            drop(unsafe { Box::from_raw(handle) });
        }
    }));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_string_free(value: *mut c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !value.is_null() {
            drop(unsafe { CString::from_raw(value) });
        }
    }));
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn sb_root_status() -> *mut c_char {
    use super::root;
    use std::os::unix::ffi::OsStrExt;

    let text = catch_unwind(AssertUnwindSafe(|| {
        if !root::enabled() {
            return "未启用 Root 模式".into();
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        let available = std::env::split_paths(&path).any(|dir| {
            CString::new(dir.join("su").as_os_str().as_bytes())
                .is_ok_and(|file| unsafe { libc::access(file.as_ptr(), libc::X_OK) == 0 })
        });
        if !available {
            return "未找到可执行的 su，无法查询 Root 状态".into();
        }
        let result = root::run(
            "if [ -f /data/adb/singboard/singboard.sh ]; then sh /data/adb/singboard/singboard.sh status; else echo not_installed; fi",
        );
        result.unwrap_or_else(|err| format!("读取失败：{err}"))
    }))
    .unwrap_or_else(|_| "读取失败：内部执行异常".into());
    output(text)
}
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_config_request(
    dir: *const c_char,
    request: *const c_char,
) -> *mut c_char {
    let envelope = match catch_unwind(AssertUnwindSafe(|| {
        let result = (|| {
            let dir = unsafe { input(dir) }.ok_or("无效的应用目录")?;
            let command = unsafe { input(request) }.ok_or("无效的配置操作")?;
            super::runtime().block_on(super::android_config::request(dir.into(), command))
        })();
        match result {
            Ok(data) => serde_json::json!({"data":data}),
            Err(error) => serde_json::json!({"error":error}),
        }
    })) {
        Ok(envelope) => envelope,
        Err(_) => serde_json::json!({"error": "内部执行异常"}),
    };
    output(envelope.to_string())
}
