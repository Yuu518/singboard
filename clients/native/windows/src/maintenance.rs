use crate::service;
use singboard_stream::{settings::Settings, update};
use std::{fs, path::Path, process::Command};

pub const APPLY: &str = "--apply-native-update";
pub const CLEANUP: &str = "--finish-native-update";

pub fn preferences() -> Result<Settings, String> {
    Settings::load(&service::data_dir()?)
}

pub fn launch_installer(source: &Path, digest: &str) -> Result<(), String> {
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    let worker = source.with_file_name("singboard-updater.exe");
    fs::copy(&target, &worker).map_err(|e| e.to_string())?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut command = Command::new(worker);
    command
        .arg(APPLY)
        .arg(&target)
        .arg(std::process::id().to_string())
        .arg(digest);
    if let Some(path) = args
        .iter()
        .position(|a| a == "--wallpaper")
        .and_then(|i| args.get(i + 1))
    {
        command.arg("--wallpaper").arg(path);
    }
    command.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn apply(
    target: &Path,
    pid: u32,
    digest: &str,
    restart_args: &[std::ffi::OsString],
) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject},
    };
    let worker = std::env::current_exe().map_err(|e| e.to_string())?;
    if digest.len() != 64
        || !digest.bytes().all(|c| c.is_ascii_hexdigit())
        || worker.parent() != Some(service::data_dir()?.join("updates").join(digest).as_path())
        || !target.is_absolute()
    {
        return Err("更新目录或校验信息无效".into());
    }
    let staged = worker.with_file_name(update::staged_filename());
    let handle = unsafe { OpenProcess(0x00100000, 0, pid) };
    if !handle.is_null() {
        let status = unsafe {
            let result = WaitForSingleObject(handle, 60000);
            CloseHandle(handle);
            result
        };
        if status != WAIT_OBJECT_0 {
            return Err("面板仍在运行，已取消替换".into());
        }
    }
    let before = fs::read(&worker).map_err(|e| e.to_string())?;
    let content = fs::read(&staged).map_err(|e| e.to_string())?;
    if let Err(e) = replace_update(target, &before, &content, digest) {
        if fs::read(target).ok().as_ref() == Some(&before) {
            let _ = Command::new(target).args(restart_args).spawn();
        }
        return Err(format!("更新失败：{e}\n下载位置：{}", staged.display()));
    }
    if let Err(e) = Command::new(target)
        .args(restart_args)
        .arg(CLEANUP)
        .arg(digest)
        .spawn()
    {
        singboard_stream::config_files::replace_checked(target, &before, Some(&content))
            .map_err(|error| format!("启动失败：{e}。恢复旧版失败：{error}"))?;
        Command::new(target)
            .args(restart_args)
            .spawn()
            .map_err(|error| format!("已恢复旧版，请手动启动：{error}"))?;
        return Err(format!("新版启动失败，已恢复旧版：{e}"));
    }
    let _ = fs::remove_file(staged);
    Ok(())
}

pub fn finish(digest: &str) {
    if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
        return;
    }
    let Ok(root) = service::data_dir() else {
        return;
    };
    let dir = root.join("updates").join(digest);
    let worker = dir.join("singboard-updater.exe");
    std::thread::spawn(move || {
        if !std::env::current_exe()
            .ok()
            .and_then(|p| update::hash(&p).ok())
            .is_some_and(|hash| dir.file_name() == Some(hash.as_ref()))
        {
            return;
        }
        for _ in 0..20 {
            match fs::remove_file(&worker) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(250)),
            }
        }
        let _ = fs::remove_dir(dir);
    });
}

fn replace_update(
    target: &Path,
    before: &[u8],
    content: &[u8],
    digest: &str,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    if format!("{:x}", Sha256::digest(content)) != digest {
        return Err("下载的 EXE 已发生变化".into());
    }
    singboard_stream::config_files::replace_checked(target, content, Some(before)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::replace_update;
    use std::fs;
    #[test]
    fn installation_preserves_the_old_executable_on_mismatch() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("singboard.exe");
        fs::write(&exe, b"old").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"new"));
        assert!(replace_update(&exe, b"old", b"wrong", &digest).is_err());
        assert!(replace_update(&exe, b"stale", b"new", &digest).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"old");
        replace_update(&exe, b"old", b"new", &digest).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert_eq!(fs::read(exe.with_extension("exe.bak")).unwrap(), b"old");
    }
}
