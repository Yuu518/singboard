#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // 兼容 `singboard-service.exe service <name>` 与 `singboard-service.exe <name>`
    let service_name = match args.get(1).map(String::as_str) {
        Some("service") => args.get(2).cloned(),
        Some(other) => Some(other.to_string()),
        None => None,
    }
    .unwrap_or_else(|| "sing-box".to_string());

    if let Err(e) = singboard_service::wrapper::run_service(&service_name) {
        eprintln!("Service error: {}", e);
        std::process::exit(1);
    }
}
