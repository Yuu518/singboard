use serde::Serialize;
use std::path::{Path,PathBuf};

const MAX_SCAN_DEPTH: usize = 8;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedRuntimeFiles {
    pub base_dir: String,
    pub singbox_path: Option<String>,
    pub config_path: Option<String>,
    pub found: bool,
}

fn is_singbox_binary(file_name: &str) -> bool {
    matches!(
        file_name.to_ascii_lowercase().as_str(),
        "sing-box.exe" | "sing-box" | "singbox.exe" | "singbox"
    )
}

#[cfg(windows)]
pub(super) fn normalize_path_for_client(path: &Path) -> String {
    let raw = path.to_string_lossy().to_string();
    if let Some(stripped) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", stripped)
    } else if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        raw
    }
}

#[cfg(not(windows))]
pub(super) fn normalize_path_for_client(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn scan_dir(
    root: &Path,
    singbox_path: &mut Option<PathBuf>,
    config_path: &mut Option<PathBuf>,
) -> Result<(), String> {
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if depth > MAX_SCAN_DEPTH || (singbox_path.is_some() && config_path.is_some()) {
            continue;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if depth == 0 => {
                return Err(format!("Failed to read directory {}: {}", dir.display(), e));
            }
            Err(_) => continue,
        };
        let mut entries: Vec<PathBuf> = rd.filter_map(|entry| entry.ok().map(|e| e.path())).collect();
        entries.sort();

        for path in entries {
            if singbox_path.is_some() && config_path.is_some() {
                break;
            }
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let ft = meta.file_type();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_file() {
                if config_path.is_none()
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.eq_ignore_ascii_case("config.json"))
                        .unwrap_or(false)
                {
                    *config_path = Some(path.clone());
                }
                if singbox_path.is_none() {
                    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                        if is_singbox_binary(file_name) {
                            *singbox_path = Some(path.clone());
                        }
                    }
                }
            } else if ft.is_dir() && depth < MAX_SCAN_DEPTH {
                queue.push_back((path, depth + 1));
            }
        }
    }
    Ok(())
}

pub async fn detect_runtime_files(
    base_dir: Option<String>,
) -> Result<DetectedRuntimeFiles, String> {
    tokio::task::spawn_blocking(move || {
        let base_dir = if let Some(input) = base_dir {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                std::env::current_dir()
                    .map_err(|e| format!("Failed to get current directory: {}", e))?
            } else {
                let path = PathBuf::from(trimmed);
                if !path.exists() {
                    return Err(format!(
                        "Working directory does not exist: {}",
                        path.display()
                    ));
                }
                if !path.is_dir() {
                    return Err(format!(
                        "Working directory is not a directory: {}",
                        path.display()
                    ));
                }
                path
            }
        } else {
            std::env::current_dir()
                .map_err(|e| format!("Failed to get current directory: {}", e))?
        };

        let base_dir = std::fs::canonicalize(&base_dir).unwrap_or(base_dir);
        let mut singbox_path = None;
        let mut config_path = None;

        scan_dir(&base_dir, &mut singbox_path, &mut config_path)?;
        let found = singbox_path.is_some() && config_path.is_some();

        Ok(DetectedRuntimeFiles {
            base_dir: normalize_path_for_client(&base_dir),
            singbox_path: singbox_path
                .as_ref()
                .map(|p| normalize_path_for_client(p.as_path())),
            config_path: config_path
                .as_ref()
                .map(|p| normalize_path_for_client(p.as_path())),
            found,
        })
    })
    .await
    .map_err(|e| format!("Failed to run detection task: {}", e))?
}

