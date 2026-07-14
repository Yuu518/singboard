use std::fs;
use std::path::{Path, PathBuf};

pub const SERVICE_ERROR_LOG_NAME: &str = "singbox_last_error.log";

fn is_log_dir_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("log") || name.eq_ignore_ascii_case("logs")
}

fn find_log_dir(base_dir: &Path) -> Option<PathBuf> {
    for candidate in ["log", "logs"] {
        let path = base_dir.join(candidate);
        if path.is_dir() {
            return Some(path);
        }
    }

    let mut stack = vec![base_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if is_log_dir_name(name) {
                    return Some(path);
                }
            }
            stack.push(path);
        }
    }

    None
}

fn resolve_service_base_dir(service_name: &str) -> Option<PathBuf> {
    let (singbox_path, config_path, working_dir) = read_service_params(service_name).ok()?;

    if !working_dir.trim().is_empty() {
        let path = PathBuf::from(working_dir.trim());
        if path.is_dir() {
            return Some(path);
        }
    }

    let config = Path::new(config_path.trim());
    if let Some(parent) = config.parent() {
        if parent.is_dir() {
            return Some(parent.to_path_buf());
        }
    }

    let singbox = Path::new(singbox_path.trim());
    if let Some(parent) = singbox.parent() {
        if parent.is_dir() {
            return Some(parent.to_path_buf());
        }
    }

    None
}

pub fn resolve_service_error_log_path(service_name: &str) -> PathBuf {
    let base_dir = resolve_service_base_dir(service_name)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        })
        .unwrap_or_default();

    let log_dir = find_log_dir(&base_dir).unwrap_or(base_dir);
    log_dir.join(SERVICE_ERROR_LOG_NAME)
}

pub fn write_service_params(
    service_name: &str,
    singbox_path: &str,
    config_path: &str,
    working_dir: &str,
) -> Result<(), String> {
    use winreg::RegKey;
    use winreg::enums::*;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key_path = format!(
        "SYSTEM\\CurrentControlSet\\Services\\{}\\Parameters",
        service_name
    );
    let (key, _) = hklm
        .create_subkey(&key_path)
        .map_err(|e| format!("Failed to create registry key: {}", e))?;
    key.set_value("SingboxPath", &singbox_path)
        .map_err(|e| format!("Failed to write SingboxPath: {}", e))?;
    key.set_value("ConfigPath", &config_path)
        .map_err(|e| format!("Failed to write ConfigPath: {}", e))?;
    key.set_value("WorkingDir", &working_dir)
        .map_err(|e| format!("Failed to write WorkingDir: {}", e))?;
    Ok(())
}

pub fn read_service_params(service_name: &str) -> Result<(String, String, String), String> {
    use winreg::RegKey;
    use winreg::enums::*;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key_path = format!(
        "SYSTEM\\CurrentControlSet\\Services\\{}\\Parameters",
        service_name
    );
    let key = hklm
        .open_subkey(&key_path)
        .map_err(|e| format!("Failed to open registry key: {}", e))?;
    let singbox_path: String = key
        .get_value("SingboxPath")
        .map_err(|e| format!("Failed to read SingboxPath: {}", e))?;
    let config_path: String = key
        .get_value("ConfigPath")
        .map_err(|e| format!("Failed to read ConfigPath: {}", e))?;
    let working_dir: String = key.get_value("WorkingDir").unwrap_or_default();
    Ok((singbox_path, config_path, working_dir))
}

pub fn read_service_error_log(service_name: &str) -> Result<String, String> {
    let log_path = resolve_service_error_log_path(service_name);
    std::fs::read_to_string(&log_path).map_err(|e| format!("Failed to read error log: {}", e))
}
