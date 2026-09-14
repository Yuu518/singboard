#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(e) = singboard_service::wrapper::run_service(singboard_service::SERVICE_NAME) {
        eprintln!("Service error: {}", e);
        std::process::exit(1);
    }
}
