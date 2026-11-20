//! Startup synchronization and migration of the app-owned Windows service.
pub use super::managed::*;
use super::{SERVICE_NAME, elevation, helper, scm};
use tauri::Manager;

static STARTUP_SYNC: tokio::sync::OnceCell<Result<String, String>> =
    tokio::sync::OnceCell::const_new();

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
                Ok(match helper::sync_needed(SERVICE_NAME)? {
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
