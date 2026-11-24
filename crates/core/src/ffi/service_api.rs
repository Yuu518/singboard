use super::{input, output};
use crate::service_api::{ApiError, ServiceApi};
use serde_json::json;
use std::collections::HashMap;
use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, LazyLock, Mutex};
use tokio_util::sync::CancellationToken;

struct Session {
    backend: tokio::sync::Mutex<ServiceApi>,
    closed: CancellationToken,
}

#[derive(Default)]
struct Sessions {
    last_id: u64,
    open: HashMap<u64, Arc<Session>>,
}

static SESSIONS: LazyLock<Mutex<Sessions>> = LazyLock::new(Mutex::default);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_service_api_open(dir: *const c_char) -> u64 {
    catch_unwind(|| {
        let Some(dir) = (unsafe { input(dir) }).filter(|s| !s.is_empty()) else {
            return 0;
        };
        let mut sessions = SESSIONS.lock().unwrap();
        let Some(id) = sessions.last_id.checked_add(1) else {
            return 0;
        };
        let session = Session {
            backend: tokio::sync::Mutex::new(ServiceApi::new(dir.into())),
            closed: CancellationToken::new(),
        };
        sessions.last_id = id;
        sessions.open.insert(id, Arc::new(session));
        id
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sb_service_api_request(id: u64, request: *const c_char) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let session = SESSIONS
            .lock()
            .unwrap()
            .open
            .get(&id)
            .cloned()
            .ok_or_else(|| ApiError::invalid("会话已关闭"))?;
        let request = unsafe { input(request) }.ok_or_else(|| ApiError::invalid("无效的请求"))?;
        let command = serde_json::from_str(request)
            .map_err(|_| ApiError::invalid("无效的 Service API 操作或参数"))?;
        crate::runtime().block_on(async {
            tokio::select! {
                biased;
                _ = session.closed.cancelled() => Err(ApiError::invalid("会话已关闭")),
                result = async { session.backend.lock().await.execute(command).await } => result,
            }
        })
    }));
    let reply = match result {
        Ok(Ok(data)) => json!({"data": data}),
        Ok(Err(error)) => json!({"error": error}),
        Err(_) => json!({"error": {"code":"internal", "message":"内部执行异常"}}),
    };
    output(reply.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn sb_service_api_close(id: u64) {
    let _ = catch_unwind(|| {
        if let Some(session) = SESSIONS.lock().unwrap().open.remove(&id) {
            session.closed.cancel();
        }
    });
}
