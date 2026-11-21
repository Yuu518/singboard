use std::path::{Path, PathBuf};

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub fn config_backup_path(path: &Path) -> PathBuf {
    sibling_path(path, ".bak")
}

fn stage(path: &Path, content: &[u8]) -> Result<tempfile::NamedTempFile, String> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged =
        tempfile::NamedTempFile::new_in(dir).map_err(|e| format!("创建临时文件失败：{e}"))?;
    staged
        .write_all(content)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|e| format!("写入临时文件失败：{e}"))?;
    Ok(staged)
}

pub(super) fn write_atomic(path: &Path, content: &[u8]) -> Result<(), String> {
    stage(path, content)?
        .persist(path)
        .map_err(|e| format!("替换配置文件失败：{e}"))?;
    Ok(())
}

pub fn replace_with_backup(path: &Path, content: &[u8]) -> Result<bool, String> {
    replace_checked(path, content, None)
}

fn lock_config(path: &Path) -> Result<std::fs::File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let lock_path = sibling_path(path, ".singboard.lock");
    let mut last_error = None;
    for attempt in 0..10 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_DELETE_ON_CLOSE)
            .open(&lock_path)
        {
            Ok(file) => return Ok(file),
            Err(e) => last_error = Some(e),
        }
    }
    Err(format!("无法锁定配置文件：{}", last_error.unwrap()))
}

pub fn replace_checked(
    path: &Path,
    content: &[u8],
    expected: Option<&[u8]>,
) -> Result<bool, String> {
    let _lock = lock_config(path)?;
    match std::fs::read(path) {
        Ok(previous) if expected.is_some_and(|bytes| bytes != previous) => {
            return Err("磁盘上的配置已更改。请复制要保留的编辑，再重新打开文件。".into());
        }
        Ok(previous) if previous == content => return Ok(false),
        Ok(_) => replace_existing(path, content)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && expected.is_none() => {
            write_atomic(path, content)?
        }
        Err(e) => return Err(format!("读取原配置失败：{e}")),
    }
    Ok(true)
}

fn replace_existing(path: &Path, content: &[u8]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    const ERROR_UNABLE_TO_MOVE_REPLACEMENT: i32 = 1176;
    let staged = stage(path, content)?.into_temp_path();
    let backup_path = config_backup_path(path);
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let target = wide(path);
    let replacement = wide(&staged);
    let backup = wide(&backup_path);
    let success = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            replacement.as_ptr(),
            backup.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if success == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT) {
            match staged.persist(path) {
                Ok(()) => return Ok(()),
                Err(persist_err) => {
                    let restored = !path.exists() && std::fs::rename(&backup_path, path).is_ok();
                    if !restored {
                        if let Ok(kept) = persist_err.path.keep() {
                            return Err(format!(
                                "替换配置文件失败：{error}；新内容已保留在 {}",
                                kept.display()
                            ));
                        }
                    }
                    return Err(format!("替换配置文件失败：{error}"));
                }
            }
        }
        return Err(format!("替换配置文件失败：{error}"));
    }
    let _ = staged.keep();
    Ok(())
}

pub fn swap_with_backup(path: &Path) -> Result<(), String> {
    let _lock = lock_config(path)?;
    let backup = config_backup_path(path);
    let previous = match std::fs::read(&backup) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("没有可恢复的上一版本".into());
        }
        Err(e) => return Err(format!("读取上一版本失败：{e}")),
    };
    if path
        .try_exists()
        .map_err(|e| format!("读取当前配置失败：{e}"))?
    {
        replace_existing(path, &previous)
    } else {
        write_atomic(path, &previous)?;
        std::fs::remove_file(&backup).map_err(|e| format!("清理上一版本失败：{e}"))
    }
}

fn resolve_working_dir(working_dir: Option<&str>, config_path: &str) -> Result<PathBuf, String> {
    if let Some(dir) = working_dir.map(str::trim).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(dir);
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
        return Ok(path);
    }

    if let Some(parent) = Path::new(config_path).parent() {
        if !parent.as_os_str().is_empty() {
            return Ok(parent.to_path_buf());
        }
    }

    std::env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))
}

pub async fn run_singbox_check(
    singbox_path: &str,
    config_path: &str,
    working_dir: Option<&str>,
) -> Result<String, String> {
    let cwd = resolve_working_dir(working_dir, config_path)?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(&singbox_path)
            .args(["check", "-c", &config_path])
            .current_dir(&cwd)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .env("NO_COLOR", "1")
            .creation_flags(0x08000000)
            .output(),
    )
    .await
    .map_err(|_| "sing-box 配置校验超时（30 秒）".to_string())?
    .map_err(|e| format!("无法运行 sing-box 校验：{}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok("Configuration is valid".into())
    } else {
        Err(format!("{}\n{}", stdout, stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "singboard-config-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn replacing_keeps_the_previous_version_as_backup() {
        let dir = temp_dir("replace");
        let path = dir.join("remote.json");
        std::fs::write(&path, b"old").unwrap();

        assert!(replace_with_backup(&path, b"new").unwrap());

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read(config_backup_path(&path)).unwrap(), b"old");
        assert!(!sibling_path(&path, ".tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replacing_with_identical_content_keeps_the_older_backup() {
        let dir = temp_dir("same");
        let path = dir.join("remote.json");
        std::fs::write(&path, b"current").unwrap();
        std::fs::write(config_backup_path(&path), b"older").unwrap();

        assert!(!replace_with_backup(&path, b"current").unwrap());

        assert_eq!(std::fs::read(config_backup_path(&path)).unwrap(), b"older");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_locked_target_keeps_both_versions() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, b"current").unwrap();
        let backup = config_backup_path(&path);
        std::fs::write(&backup, b"older").unwrap();
        let _reader = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(replace_with_backup(&path, b"next").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"current");
        assert_eq!(std::fs::read(&backup).unwrap(), b"older");
        assert_eq!(dir.path().read_dir().unwrap().count(), 2);
    }

    #[test]
    fn first_download_writes_without_a_backup() {
        let dir = temp_dir("first");
        let path = dir.join("remote.json");

        replace_with_backup(&path, b"fresh").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"fresh");
        assert!(!config_backup_path(&path).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn restoring_swaps_current_and_previous_so_it_can_be_undone() {
        let dir = temp_dir("restore");
        let path = dir.join("remote.json");
        std::fs::write(&path, b"broken").unwrap();
        std::fs::write(config_backup_path(&path), b"working").unwrap();

        swap_with_backup(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"working");
        assert_eq!(std::fs::read(config_backup_path(&path)).unwrap(), b"broken");

        swap_with_backup(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        assert!(!sibling_path(&path, ".bak.swap").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn restoring_without_a_backup_reports_it() {
        let dir = temp_dir("missing");
        let path = dir.join("remote.json");
        std::fs::write(&path, b"only").unwrap();

        assert_eq!(swap_with_backup(&path).unwrap_err(), "没有可恢复的上一版本");
        assert_eq!(std::fs::read(&path).unwrap(), b"only");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
