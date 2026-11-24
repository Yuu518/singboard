use super::service_api::{close_session, open_session, request_session};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

type JniEnv = *mut *const *const c_void;
type JClass = *mut c_void;
type JString = *mut c_void;
type JSize = i32;
type JChar = u16;
type JBoolean = u8;

const IDX_NEW_STRING: usize = 163;
const IDX_GET_STRING_LENGTH: usize = 164;
const IDX_GET_STRING_CHARS: usize = 165;
const IDX_RELEASE_STRING_CHARS: usize = 166;

unsafe fn read_jstring(env: JniEnv, value: JString) -> Option<String> {
    if env.is_null() || value.is_null() {
        return None;
    }
    let vtable = unsafe { *env };
    if vtable.is_null() {
        return None;
    }
    let get_len: unsafe extern "system" fn(JniEnv, JString) -> JSize =
        unsafe { std::mem::transmute(*vtable.add(IDX_GET_STRING_LENGTH)) };
    let get_chars: unsafe extern "system" fn(JniEnv, JString, *mut JBoolean) -> *const JChar =
        unsafe { std::mem::transmute(*vtable.add(IDX_GET_STRING_CHARS)) };
    let release_chars: unsafe extern "system" fn(JniEnv, JString, *const JChar) =
        unsafe { std::mem::transmute(*vtable.add(IDX_RELEASE_STRING_CHARS)) };

    let len = unsafe { get_len(env, value) };
    if len < 0 {
        return None;
    }
    if len == 0 {
        return Some(String::new());
    }
    let chars = unsafe { get_chars(env, value, ptr::null_mut()) };
    if chars.is_null() {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(chars, len as usize) };
    let text = String::from_utf16_lossy(slice);
    unsafe { release_chars(env, value, chars) };
    Some(text)
}

unsafe fn make_jstring(env: JniEnv, value: &str) -> JString {
    if env.is_null() {
        return ptr::null_mut();
    }
    let vtable = unsafe { *env };
    if vtable.is_null() {
        return ptr::null_mut();
    }
    let new_string: unsafe extern "system" fn(JniEnv, *const JChar, JSize) -> JString =
        unsafe { std::mem::transmute(*vtable.add(IDX_NEW_STRING)) };
    let utf16: Vec<u16> = value.encode_utf16().collect();
    let len = i32::try_from(utf16.len()).unwrap_or(0);
    unsafe { new_string(env, utf16.as_ptr(), len) }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_Yuu518_singboard_TaildropNative_open(
    env: JniEnv,
    _class: JClass,
    dir: JString,
) -> i64 {
    catch_unwind(AssertUnwindSafe(|| {
        let Some(dir) = (unsafe { read_jstring(env, dir) }) else {
            return 0;
        };
        open_session(&dir) as i64
    }))
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_Yuu518_singboard_TaildropNative_request(
    env: JniEnv,
    _class: JClass,
    handle: i64,
    request: JString,
) -> JString {
    let reply = catch_unwind(AssertUnwindSafe(|| {
        let Some(request) = (unsafe { read_jstring(env, request) }) else {
            return serde_json::json!({"error": {"code": "invalidArgument", "message": "无效的请求"}}).to_string();
        };
        if handle <= 0 {
            return serde_json::json!({"error": {"code": "invalidArgument", "message": "会话已关闭"}}).to_string();
        }
        request_session(handle as u64, &request)
    }))
    .unwrap_or_else(|_| {
        serde_json::json!({"error": {"code": "internal", "message": "内部执行异常"}}).to_string()
    });
    unsafe { make_jstring(env, &reply) }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_Yuu518_singboard_TaildropNative_close(
    _env: JniEnv,
    _class: JClass,
    handle: i64,
) {
    if handle > 0 {
        let _ = catch_unwind(|| close_session(handle as u64));
    }
}
