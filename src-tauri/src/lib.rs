pub mod commands;
#[cfg(windows)]
pub mod desktop;
#[cfg(target_os = "android")]
pub mod native;
pub mod root;
#[cfg(windows)]
pub mod service;

#[cfg(target_os = "android")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use commands::{app, binary, config, network, self_update, service, srs, update};

    tauri::Builder::default()
        .plugin(native::init())
        .invoke_handler(tauri::generate_handler![
            app::root_check,
            app::set_root_enabled,
            app::set_system_bars,
            app::system_insets,
            service::service_status,
            service::service_start,
            service::service_stop,
            service::service_restart,
            service::service_install,
            service::service_component_sync,
            service::service_error_log,
            service::service_boot_hook_exists,
            service::service_create_boot_hook,
            service::service_delete_boot_hook,
            config::validate_config,
            config::detect_runtime_files,
            binary::get_singbox_version,
            binary::get_file_hash,
            srs::srs_match_provider,
            srs::srs_list_provider,
            network::fetch_url,
            network::http_ping,
            network::dns_query,
            update::check_core_update,
            update::platform::probe_asset_core_hash,
            update::platform::perform_core_update,
            update::open_release_page,
            self_update::check_panel_update,
            self_update::perform_panel_update,
            self_update::cleanup_panel_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
