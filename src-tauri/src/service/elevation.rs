pub use super::admin::{ADMIN_FLAG, CANCELLED, Operation, quote_arg};
use super::admin;
use serde_json::Value;
use singboard_service::ipc;
use std::path::PathBuf;
use tauri::Manager;

pub async fn request(app: &tauri::AppHandle, operation: Operation) -> Result<Value, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    request_with_context(
        operation,
        data_dir,
        ipc::current_user_sid().map_err(|e| e.to_string())?,
        Some(app),
    )
    .await
}

pub async fn request_with_context(
    operation: Operation,
    data_dir: PathBuf,
    user_sid: String,
    app: Option<&tauri::AppHandle>,
) -> Result<Value, String> {
    admin::request(operation, data_dir, user_sid, |phase| {
        if let Some(app) = app {
            let phase = if phase == "restart" {
                "restart"
            } else {
                "replace"
            };
            crate::commands::update::emit_progress(
                app,
                crate::commands::update::CORE_PROGRESS_EVENT,
                phase,
                0,
                0,
            );
        }
    })
    .await
}

pub fn run_child(name: &str, parent_pid: u32) -> Result<(), String> {
    admin::run_child(
        name,
        parent_pid,
        |operation, _, _, _| {
            match operation {
                Operation::ReplacePanel {
                    source,
                    target,
                    hash,
                } => {
                    use sha2::{Digest, Sha256};
                    let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
                    if format!("{:x}", Sha256::digest(&bytes)) != hash {
                        return Err("面板更新文件已发生变化".into());
                    }
                    crate::commands::self_update::overwrite_bytes_with_backup(&bytes, &target)
                        .map_err(|e| e.to_string())?;
                }
                _ => return Err("未知的管理操作".into()),
            }
            Ok(Value::Null)
        },
    )
}
