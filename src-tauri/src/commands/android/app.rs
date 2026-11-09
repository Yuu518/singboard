use crate::root;

#[tauri::command]
pub async fn root_check() -> bool {
    tokio::task::spawn_blocking(root::probe)
        .await
        .unwrap_or(false)
}

#[tauri::command]
pub fn set_root_enabled(enabled: bool) {
    root::set_enabled(enabled);
}

#[tauri::command]
pub async fn set_system_bars(app: tauri::AppHandle, dark: bool, color: Option<String>) {
    let _ = crate::native::set_system_bars(&app, dark, color);
}

#[tauri::command]
pub async fn system_insets(app: tauri::AppHandle) -> Option<crate::native::SystemInsets> {
    crate::native::system_insets(&app).ok()
}
