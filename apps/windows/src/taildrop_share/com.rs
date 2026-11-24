use super::registry::DELEGATE_CLSID;
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
    mpsc,
};
use windows_sys::Win32::Foundation::{
    CLASS_E_NOAGGREGATION, E_NOINTERFACE, E_POINTER, POINT, S_OK,
};
use windows_sys::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoInitializeEx, CoRegisterClassObject,
    CoRevokeClassObject, CoTaskMemFree, CoUninitialize, REGCLS_MULTIPLEUSE, REGCLS_SINGLEUSE,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Shell::SIGDN_FILESYSPATH;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, MSG, PM_NOREMOVE, PeekMessageW, PostQuitMessage,
    PostThreadMessageW, SetTimer, TranslateMessage, WM_QUIT, WM_TIMER,
};
use windows_sys::core::{GUID, HRESULT, PCWSTR, PWSTR};

const IID_IUNKNOWN: GUID = GUID {
    data1: 0x00000000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
const IID_ICLASS_FACTORY: GUID = GUID {
    data1: 0x00000001,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
const IID_IEXECUTE_COMMAND: GUID = GUID {
    data1: 0x7F9185B0,
    data2: 0xCB92,
    data3: 0x43C5,
    data4: [0x80, 0xA9, 0x92, 0x27, 0x7A, 0x4F, 0x7B, 0x54],
};
const IID_IOBJECT_WITH_SELECTION: GUID = GUID {
    data1: 0x1C9CD5BB,
    data2: 0x98E9,
    data3: 0x4491,
    data4: [0xA6, 0x0F, 0x31, 0xAA, 0xCC, 0x72, 0xB8, 0x3C],
};

fn guid_eq(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

pub struct DelegatePayload {
    pub paths: Vec<PathBuf>,
    pub position: Option<POINT>,
}

#[derive(Clone)]
enum ExecuteSink {
    OneShot,
    Resident(Arc<dyn Fn(DelegatePayload) + Send + Sync>),
}

static RESULT_SLOT: Mutex<Option<DelegatePayload>> = Mutex::new(None);

#[repr(C)]
struct ExecuteCommandVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    set_key_state: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    set_parameters: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    set_position: unsafe extern "system" fn(*mut c_void, POINT) -> HRESULT,
    set_show_window: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
    set_no_show_ui: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
    set_directory: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    execute: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

#[repr(C)]
struct ObjectWithSelectionVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    set_selection: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    get_selection: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct DelegateObject {
    exec_vtbl: *const ExecuteCommandVtbl,
    sel_vtbl: *const ObjectWithSelectionVtbl,
    refs: AtomicU32,
    paths: Mutex<Vec<PathBuf>>,
    position: Mutex<Option<POINT>>,
    sink: ExecuteSink,
}

static EXEC_VTBL: ExecuteCommandVtbl = ExecuteCommandVtbl {
    query_interface: exec_qi,
    add_ref: exec_add_ref,
    release: exec_release,
    set_key_state: exec_set_key_state,
    set_parameters: exec_set_parameters,
    set_position: exec_set_position,
    set_show_window: exec_set_show_window,
    set_no_show_ui: exec_set_no_show_ui,
    set_directory: exec_set_directory,
    execute: exec_execute,
};

static SEL_VTBL: ObjectWithSelectionVtbl = ObjectWithSelectionVtbl {
    query_interface: sel_qi,
    add_ref: sel_add_ref,
    release: sel_release,
    set_selection: sel_set_selection,
    get_selection: sel_get_selection,
};

unsafe fn from_exec(this: *mut c_void) -> *mut DelegateObject {
    this as *mut DelegateObject
}

unsafe fn from_sel(this: *mut c_void) -> *mut DelegateObject {
    let offset = std::mem::offset_of!(DelegateObject, sel_vtbl);
    unsafe { (this as *mut u8).sub(offset) as *mut DelegateObject }
}

unsafe fn delegate_qi(obj: *mut DelegateObject, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    unsafe { *ppv = ptr::null_mut() };
    if riid.is_null() {
        return E_NOINTERFACE;
    }
    let iid = unsafe { &*riid };
    if guid_eq(iid, &IID_IUNKNOWN) || guid_eq(iid, &IID_IEXECUTE_COMMAND) {
        unsafe { (*obj).refs.fetch_add(1, Ordering::Relaxed) };
        unsafe { *ppv = obj as *mut c_void };
        S_OK
    } else if guid_eq(iid, &IID_IOBJECT_WITH_SELECTION) {
        unsafe { (*obj).refs.fetch_add(1, Ordering::Relaxed) };
        let sel_ptr = unsafe { ptr::addr_of_mut!((*obj).sel_vtbl) as *mut c_void };
        unsafe { *ppv = sel_ptr };
        S_OK
    } else {
        E_NOINTERFACE
    }
}

unsafe fn delegate_add_ref(obj: *mut DelegateObject) -> u32 {
    unsafe { (*obj).refs.fetch_add(1, Ordering::Relaxed) + 1 }
}

unsafe fn delegate_release(obj: *mut DelegateObject) -> u32 {
    let prev = unsafe { (*obj).refs.fetch_sub(1, Ordering::Release) };
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(unsafe { Box::from_raw(obj) });
    }
    prev.saturating_sub(1)
}

unsafe extern "system" fn exec_qi(this: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    unsafe { delegate_qi(from_exec(this), riid, ppv) }
}
unsafe extern "system" fn exec_add_ref(this: *mut c_void) -> u32 {
    unsafe { delegate_add_ref(from_exec(this)) }
}
unsafe extern "system" fn exec_release(this: *mut c_void) -> u32 {
    unsafe { delegate_release(from_exec(this)) }
}
unsafe extern "system" fn exec_set_key_state(_this: *mut c_void, _state: u32) -> HRESULT {
    S_OK
}
unsafe extern "system" fn exec_set_parameters(_this: *mut c_void, _params: PCWSTR) -> HRESULT {
    S_OK
}
unsafe extern "system" fn exec_set_position(this: *mut c_void, pt: POINT) -> HRESULT {
    let obj = unsafe { &*from_exec(this) };
    if let Ok(mut guard) = obj.position.lock() {
        *guard = Some(pt);
    }
    S_OK
}
unsafe extern "system" fn exec_set_show_window(_this: *mut c_void, _show: i32) -> HRESULT {
    S_OK
}
unsafe extern "system" fn exec_set_no_show_ui(_this: *mut c_void, _no_ui: i32) -> HRESULT {
    S_OK
}
unsafe extern "system" fn exec_set_directory(_this: *mut c_void, _dir: PCWSTR) -> HRESULT {
    S_OK
}
unsafe extern "system" fn exec_execute(this: *mut c_void) -> HRESULT {
    let obj = unsafe { &*from_exec(this) };
    let paths = obj.paths.lock().map(|g| g.clone()).unwrap_or_default();
    let position = obj.position.lock().ok().and_then(|g| *g);
    let payload = DelegatePayload { paths, position };
    match &obj.sink {
        ExecuteSink::OneShot => {
            if let Ok(mut slot) = RESULT_SLOT.lock() {
                *slot = Some(payload);
            }
            unsafe { PostQuitMessage(0) };
        }
        ExecuteSink::Resident(handler) => {
            handler(payload);
        }
    }
    S_OK
}

unsafe extern "system" fn sel_qi(this: *mut c_void, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    unsafe { delegate_qi(from_sel(this), riid, ppv) }
}
unsafe extern "system" fn sel_add_ref(this: *mut c_void) -> u32 {
    unsafe { delegate_add_ref(from_sel(this)) }
}
unsafe extern "system" fn sel_release(this: *mut c_void) -> u32 {
    unsafe { delegate_release(from_sel(this)) }
}
unsafe extern "system" fn sel_set_selection(this: *mut c_void, psia: *mut c_void) -> HRESULT {
    let obj = unsafe { &*from_sel(this) };
    let extracted = unsafe { extract_paths_from_shell_item_array(psia) };
    if let Ok(mut guard) = obj.paths.lock() {
        *guard = extracted;
    }
    S_OK
}
unsafe extern "system" fn sel_get_selection(
    _this: *mut c_void,
    _riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if !ppv.is_null() {
        unsafe { *ppv = ptr::null_mut() };
    }
    E_NOINTERFACE
}

unsafe fn extract_paths_from_shell_item_array(psia: *mut c_void) -> Vec<PathBuf> {
    if psia.is_null() {
        return Vec::new();
    }
    let vtbl = unsafe { *(psia as *const *const *const c_void) };
    if vtbl.is_null() {
        return Vec::new();
    }
    let get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT =
        unsafe { std::mem::transmute(*vtbl.add(7)) };
    let get_item_at: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT =
        unsafe { std::mem::transmute(*vtbl.add(8)) };

    let mut count = 0u32;
    if unsafe { get_count(psia, &mut count) } < 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count as usize);
    for idx in 0..count {
        let mut item: *mut c_void = ptr::null_mut();
        if unsafe { get_item_at(psia, idx, &mut item) } < 0 || item.is_null() {
            continue;
        }
        let item_vtbl = unsafe { *(item as *const *const *const c_void) };
        if !item_vtbl.is_null() {
            let release_item: unsafe extern "system" fn(*mut c_void) -> u32 =
                unsafe { std::mem::transmute(*item_vtbl.add(2)) };
            let get_display_name: unsafe extern "system" fn(*mut c_void, i32, *mut PWSTR) -> HRESULT =
                unsafe { std::mem::transmute(*item_vtbl.add(5)) };
            let mut raw: PWSTR = ptr::null_mut();
            if unsafe { get_display_name(item, SIGDN_FILESYSPATH, &mut raw) } >= 0 && !raw.is_null()
            {
                let mut len = 0usize;
                while unsafe { *raw.add(len) } != 0 {
                    len += 1;
                }
                let slice = unsafe { std::slice::from_raw_parts(raw, len) };
                let path = PathBuf::from(OsString::from_wide(slice));
                unsafe { CoTaskMemFree(raw as *const c_void) };
                if path.is_file() {
                    out.push(path);
                }
            }
            unsafe { release_item(item) };
        }
    }
    out
}

#[repr(C)]
struct ClassFactoryVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    create_instance:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    lock_server: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
}

#[repr(C)]
struct ClassFactory {
    vtbl: *const ClassFactoryVtbl,
    refs: AtomicU32,
    sink: ExecuteSink,
}

static FACTORY_VTBL: ClassFactoryVtbl = ClassFactoryVtbl {
    query_interface: factory_qi,
    add_ref: factory_add_ref,
    release: factory_release,
    create_instance: factory_create_instance,
    lock_server: factory_lock_server,
};

unsafe extern "system" fn factory_qi(
    this: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    unsafe { *ppv = ptr::null_mut() };
    if riid.is_null() {
        return E_NOINTERFACE;
    }
    let iid = unsafe { &*riid };
    if guid_eq(iid, &IID_IUNKNOWN) || guid_eq(iid, &IID_ICLASS_FACTORY) {
        unsafe { (*(this as *mut ClassFactory)).refs.fetch_add(1, Ordering::Relaxed) };
        unsafe { *ppv = this };
        S_OK
    } else {
        E_NOINTERFACE
    }
}

unsafe extern "system" fn factory_add_ref(this: *mut c_void) -> u32 {
    unsafe { (*(this as *mut ClassFactory)).refs.fetch_add(1, Ordering::Relaxed) + 1 }
}

unsafe extern "system" fn factory_release(this: *mut c_void) -> u32 {
    let factory = this as *mut ClassFactory;
    let prev = unsafe { (*factory).refs.fetch_sub(1, Ordering::Release) };
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(unsafe { Box::from_raw(factory) });
    }
    prev.saturating_sub(1)
}

unsafe extern "system" fn factory_create_instance(
    this: *mut c_void,
    outer: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if !outer.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    let sink = unsafe { (*(this as *mut ClassFactory)).sink.clone() };
    let obj = Box::into_raw(Box::new(DelegateObject {
        exec_vtbl: &EXEC_VTBL,
        sel_vtbl: &SEL_VTBL,
        refs: AtomicU32::new(1),
        paths: Mutex::new(Vec::new()),
        position: Mutex::new(None),
        sink,
    }));
    let hr = unsafe { delegate_qi(obj, riid, ppv) };
    unsafe { delegate_release(obj) };
    hr
}

unsafe extern "system" fn factory_lock_server(_this: *mut c_void, _lock: i32) -> HRESULT {
    S_OK
}

pub fn receive_delegate_payload() -> Result<Option<DelegatePayload>, String> {
    if let Ok(mut slot) = RESULT_SLOT.lock() {
        *slot = None;
    }
    let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    if hr < 0 {
        return Err(format!("COM 初始化失败 (0x{:08x})", hr as u32));
    }

    let factory = Box::into_raw(Box::new(ClassFactory {
        vtbl: &FACTORY_VTBL,
        refs: AtomicU32::new(1),
        sink: ExecuteSink::OneShot,
    }));
    let mut cookie = 0u32;
    let reg_hr = unsafe {
        CoRegisterClassObject(
            &DELEGATE_CLSID,
            factory as *mut c_void,
            CLSCTX_LOCAL_SERVER as u32,
            REGCLS_SINGLEUSE as u32,
            &mut cookie,
        )
    };
    unsafe { factory_release(factory as *mut c_void) };
    if reg_hr < 0 {
        unsafe { CoUninitialize() };
        return Err(format!("注册 DelegateExecute COM 对象失败 (0x{:08x})", reg_hr as u32));
    }

    let timer_id = unsafe { SetTimer(ptr::null_mut(), 0, 10_000, None) };
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
        if msg.message == WM_TIMER && msg.hwnd.is_null() && msg.wParam == timer_id {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    if timer_id != 0 {
        unsafe { KillTimer(ptr::null_mut(), timer_id) };
    }
    unsafe {
        CoRevokeClassObject(cookie);
        CoUninitialize();
    }

    Ok(RESULT_SLOT.lock().ok().and_then(|mut g| g.take()))
}

pub struct ResidentHost {
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ResidentHost {
    pub fn start(handler: impl Fn(DelegatePayload) + Send + Sync + 'static) -> Option<Self> {
        let sink = ExecuteSink::Resident(Arc::new(handler));
        let (tx, rx) = mpsc::sync_channel::<Option<u32>>(1);
        let thread = std::thread::Builder::new()
            .name("taildrop-com-host".into())
            .spawn(move || {
                let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
                if hr < 0 {
                    let _ = tx.send(None);
                    return;
                }
                let mut msg: MSG = unsafe { std::mem::zeroed() };
                unsafe { PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_NOREMOVE) };

                let factory = Box::into_raw(Box::new(ClassFactory {
                    vtbl: &FACTORY_VTBL,
                    refs: AtomicU32::new(1),
                    sink,
                }));
                let mut cookie = 0u32;
                let reg_hr = unsafe {
                    CoRegisterClassObject(
                        &DELEGATE_CLSID,
                        factory as *mut c_void,
                        CLSCTX_LOCAL_SERVER as u32,
                        REGCLS_MULTIPLEUSE as u32,
                        &mut cookie,
                    )
                };
                unsafe { factory_release(factory as *mut c_void) };
                if reg_hr < 0 {
                    unsafe { CoUninitialize() };
                    let _ = tx.send(None);
                    return;
                }

                let tid = unsafe { GetCurrentThreadId() };
                let _ = tx.send(Some(tid));
                while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
                    unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                unsafe {
                    CoRevokeClassObject(cookie);
                    CoUninitialize();
                }
            })
            .ok()?;

        match rx.recv().ok().flatten() {
            Some(thread_id) => Some(Self {
                thread_id,
                thread: Some(thread),
            }),
            None => {
                let _ = thread.join();
                None
            }
        }
    }
}

impl Drop for ResidentHost {
    fn drop(&mut self) {
        if self.thread_id != 0 {
            unsafe {
                PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resident_host_starts_and_stops_cleanly() {
        let host = ResidentHost::start(|_| {}).expect("resident host should start");
        drop(host);
    }
}
