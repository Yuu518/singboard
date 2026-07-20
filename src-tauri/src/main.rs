#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;
use tauri::Manager;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);
static LAUNCHED_HIDDEN: AtomicBool = AtomicBool::new(false);

const AUTO_LAUNCH_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const AUTO_LAUNCH_NAME: &str = "singboard";

#[tauri::command]
fn set_close_to_tray(enabled: bool) {
    CLOSE_TO_TRAY.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
fn is_launched_hidden() -> bool {
    LAUNCHED_HIDDEN.load(Ordering::Relaxed)
}

#[tauri::command]
fn get_auto_launch() -> bool {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(AUTO_LAUNCH_KEY)
        .and_then(|key| key.get_value::<String, _>(AUTO_LAUNCH_NAME))
        .is_ok()
}

#[tauri::command]
fn set_auto_launch(enabled: bool) -> Result<(), String> {
    let (key, _) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .create_subkey(AUTO_LAUNCH_KEY)
        .map_err(|e| format!("打开注册表失败: {}", e))?;
    if enabled {
        let exe = env::current_exe().map_err(|e| format!("获取程序路径失败: {}", e))?;
        let value = format!("\"{}\" --hidden", exe.display());
        key.set_value(AUTO_LAUNCH_NAME, &value)
            .map_err(|e| format!("写入注册表失败: {}", e))?;
    } else if let Err(e) = key.delete_value(AUTO_LAUNCH_NAME) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("删除注册表值失败: {}", e));
        }
    }
    Ok(())
}

#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    show_window(&app);
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "service" {
        let service_name = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| "sing-box".to_string());
        // 向后兼容:旧版安装的服务仍以 `singboard.exe service <name>` 方式启动
        if let Err(e) = singboard_service::wrapper::run_service(&service_name) {
            eprintln!("Service error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|a| a == "--hidden") {
        LAUNCHED_HIDDEN.store(true, Ordering::Relaxed);
    }

    run_gui();
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = window.emit("window-visibility", true);
    }
}

// 以鼠标为锚点弹出托盘菜单窗口：默认向右上方展开，越界时翻转到左侧/下方
fn show_tray_menu(app: &tauri::AppHandle, position: tauri::PhysicalPosition<f64>) {
    if let Some(window) = app.get_webview_window("tray") {
        let size = window
            .outer_size()
            .unwrap_or_else(|_| tauri::PhysicalSize::new(0, 0));
        let (w, h) = (size.width as i32, size.height as i32);
        let (px, py) = (position.x as i32, position.y as i32);

        let monitor = app
            .monitor_from_point(position.x, position.y)
            .ok()
            .flatten()
            .or_else(|| app.primary_monitor().ok().flatten());
        let (mx, my, mw, mh) = monitor
            .map(|m| (m.position().x, m.position().y, m.size().width as i32, m.size().height as i32))
            .unwrap_or((0, 0, i32::MAX, i32::MAX));

        let x = if px + w <= mx + mw { px } else { px - w };
        let y = if py - h >= my { py - h } else { py };
        let x = x.max(mx).min(mx + mw - w);
        let y = y.max(my).min(my + mh - h);

        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn run_gui() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|a| a == "--hidden") {
                show_window(app);
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        .difference(tauri_plugin_window_state::StateFlags::VISIBLE),
                )
                .with_denylist(&["tray"])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("singboard")
                .on_tray_icon_event(move |_tray, event| {
                    if let TrayIconEvent::Click {
                        button,
                        button_state: MouseButtonState::Up,
                        position,
                        ..
                    } = event
                    {
                        match button {
                            MouseButton::Left => show_window(&app_handle),
                            MouseButton::Right => show_tray_menu(&app_handle, position),
                            _ => {}
                        }
                    }
                })
                .build(&app.handle().clone())
                .expect("tray icon");

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    if window.label() == "main" {
                        if CLOSE_TO_TRAY.load(Ordering::Relaxed) {
                            api.prevent_close();
                            let _ = window.emit("window-visibility", false);
                            let _ = window.hide();
                        } else {
                            let _ = window.app_handle().exit(0);
                        }
                    }
                }
                // 托盘菜单窗口失去焦点时自动隐藏
                tauri::WindowEvent::Focused(false) => {
                    if window.label() == "tray" {
                        let _ = window.hide();
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_close_to_tray,
            is_launched_hidden,
            get_auto_launch,
            set_auto_launch,
            show_main_window,
            quit_app,
            singboard_lib::commands::service::service_status,
            singboard_lib::commands::service::service_start,
            singboard_lib::commands::service::service_stop,
            singboard_lib::commands::service::service_restart,
            singboard_lib::commands::service::service_install,
            singboard_lib::commands::service::service_uninstall,
            singboard_lib::commands::service::service_component_sync,
            singboard_lib::commands::service::service_error_log,
            singboard_lib::commands::service::service_startup_task_exists,
            singboard_lib::commands::service::service_create_startup_task,
            singboard_lib::commands::service::service_delete_startup_task,
            singboard_lib::commands::config::read_config,
            singboard_lib::commands::config::write_config,
            singboard_lib::commands::config::validate_config,
            singboard_lib::commands::config::validate_config_content,
            singboard_lib::commands::config::detect_runtime_files,
            singboard_lib::commands::config::get_running_config_path,
            singboard_lib::commands::config::copy_to_running_config,
            singboard_lib::commands::config::get_remote_config_dir,
            singboard_lib::commands::config::get_remote_config_path,
            singboard_lib::commands::config::delete_file,
            singboard_lib::commands::binary::get_singbox_version,
            singboard_lib::commands::binary::get_file_hash,
            singboard_lib::commands::srs::srs_match,
            singboard_lib::commands::srs::srs_match_provider,
            singboard_lib::commands::srs::srs_list_provider,
            singboard_lib::commands::network::fetch_url,
            singboard_lib::commands::network::http_ping,
            singboard_lib::commands::network::dns_query,
            singboard_lib::commands::update::check_core_update,
            singboard_lib::commands::update::probe_asset_exe_hash,
            singboard_lib::commands::update::perform_core_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            use tauri_plugin_window_state::AppHandleExt;
            let state_flags = tauri_plugin_window_state::StateFlags::all()
                .difference(tauri_plugin_window_state::StateFlags::VISIBLE);
            match event {
                tauri::RunEvent::WindowEvent {
                    label,
                    event: tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_),
                    ..
                } => {
                    if label == "main" {
                        let _ = app.save_window_state(state_flags);
                    }
                }
                tauri::RunEvent::ExitRequested { .. } => {}
                _ => {}
            }
        });
}
