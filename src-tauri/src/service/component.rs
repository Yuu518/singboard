//! Startup synchronization and migration of the app-owned Windows service.
use super::{SERVICE_NAME, elevation, helper, scm};
use std::path::Path;
use tauri::Manager;

static STARTUP_SYNC: tokio::sync::OnceCell<Result<String, String>> =
    tokio::sync::OnceCell::const_new();

pub fn legacy_service_name(data_dir: &Path) -> Result<Option<String>, String> {
    use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};
    let services = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SYSTEM\CurrentControlSet\Services")
        .map_err(|e| e.to_string())?;
    let panel = std::env::current_exe().map_err(|e| e.to_string())?;
    let candidates = [
        data_dir.join("singboard-service.exe"),
        helper::deployed_helper_path(data_dir),
        panel,
    ];
    let mut found = None;
    for name in services.enum_keys() {
        let name = name.map_err(|e| e.to_string())?;
        if name.eq_ignore_ascii_case(SERVICE_NAME) {
            continue;
        }
        let Some(image) = helper::service_image_path(&name) else {
            continue;
        };
        if candidates
            .iter()
            .any(|path| helper::image_points_to(&image, path))
            && scm::read_service_params(&name).is_ok()
        {
            if found.is_some() {
                return Err("发现多个旧版 singboard 服务，无法唯一确定迁移对象".into());
            }
            found = Some(name);
        }
    }
    Ok(found)
}

// Keep a cancelled migration's existing service readable and controllable.
// Callers cannot supply a custom service name through the panel API.
pub fn current_service_name(data_dir: &Path) -> Result<String, String> {
    if scm::query_service_status(SERVICE_NAME)?.state != "not_installed" {
        Ok(SERVICE_NAME.into())
    } else {
        Ok(legacy_service_name(data_dir)?.unwrap_or_else(|| SERVICE_NAME.into()))
    }
}

pub fn app_service_name(app: &tauri::AppHandle) -> Result<String, String> {
    current_service_name(&app.path().app_data_dir().map_err(|e| e.to_string())?)
}

pub async fn sync(app: &tauri::AppHandle) -> Result<String, String> {
    // Cache cancellation/failure too: the second WebView and status polling
    // must not immediately request another elevation in the same app session.
    STARTUP_SYNC
        .get_or_init(|| async {
            let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
            let operation = tokio::task::spawn_blocking(move || {
                if scm::query_service_status(SERVICE_NAME)?.state == "not_installed" {
                    return Ok::<_, String>(
                        legacy_service_name(&data_dir)?
                            .map(|service| elevation::Operation::Migrate { service }),
                    );
                }
                Ok(match helper::sync_needed(&data_dir, SERVICE_NAME)? {
                    helper::SyncNeed::UpToDate => None,
                    _ => Some(elevation::Operation::Sync {
                        service: SERVICE_NAME.into(),
                    }),
                })
            })
            .await
            .map_err(|e| e.to_string())??;
            if let Some(operation) = operation {
                let result = elevation::request(app, operation).await?;
                Ok(result.as_str().unwrap_or("updated").to_string())
            } else {
                Ok("ok".into())
            }
        })
        .await
        .clone()
}

pub fn migrate(data_dir: &Path, old_name: &str, user_sid: &str) -> Result<(), String> {
    if legacy_service_name(data_dir)?.as_deref() != Some(old_name)
        || scm::query_service_status(SERVICE_NAME)?.state != "not_installed"
    {
        return Err("服务安装状态已变化，请重新打开面板后重试迁移".into());
    }
    let (core, config, working_dir) = scm::read_service_params(old_name)?;
    let was_running = matches!(
        scm::query_service_status(old_name)?.state.as_str(),
        "running" | "starting"
    );
    scm::stop_service(old_name)?;
    let mut created = false;
    let mut copied_task = false;
    let install = (|| {
        let deployed = helper::deploy_helper(data_dir)?;
        let bin = elevation::quote_arg(&deployed.to_string_lossy());
        scm::install_service(SERVICE_NAME, &bin, SERVICE_NAME)?;
        created = true;
        scm::write_service_params(SERVICE_NAME, &core, &config, &working_dir)?;
        singboard_service::params::write_panel_sid(SERVICE_NAME, user_sid)?;
        copied_task = scm::copy_startup_task(old_name, SERVICE_NAME)?;
        if was_running {
            scm::start_service(SERVICE_NAME)?;
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = install {
        let mut errors = vec![error];
        let mut safe_to_restore = true;
        if created {
            if let Err(e) = scm::uninstall_service(SERVICE_NAME) {
                safe_to_restore = false;
                errors.push(format!("撤销新服务失败: {e}"));
            }
        }
        if copied_task {
            if let Err(e) = scm::delete_startup_task(SERVICE_NAME) {
                errors.push(e);
            }
        }
        if was_running && safe_to_restore {
            if let Err(e) = scm::start_service(old_name) {
                errors.push(format!("恢复旧服务失败: {e}"));
            }
        }
        return Err(errors.join("; "));
    }
    // The old host and task stay available until the replacement has started.
    // A cleanup failure must not stop the successfully migrated service.
    scm::uninstall_service(old_name).map_err(|e| format!("新服务已就绪，清理旧服务失败: {e}"))?;
    scm::delete_startup_task(old_name)
        .map_err(|e| format!("服务已迁移，清理旧自启任务失败: {e}"))?;
    cleanup_legacy_files(data_dir)?;
    Ok(())
}

pub fn cleanup_legacy_files(data_dir: &Path) -> Result<(), String> {
    if legacy_service_name(data_dir)?.is_some() {
        return Ok(());
    }
    for name in ["singboard-service.exe", "singboard-service.version"] {
        match std::fs::remove_file(data_dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("清理旧服务组件失败: {e}")),
        }
    }
    Ok(())
}
