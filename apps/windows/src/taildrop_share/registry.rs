use std::env;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, GetLastError};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, QueryFullProcessImageNameW};
use windows_sys::core::GUID;
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

pub const DELEGATE_FLAG: &str = "--taildrop-delegate";
pub const SEND_FLAG: &str = "--taildrop-send";
pub const DELEGATE_CLSID_STR: &str = "{8B6B4F72-3A5E-4C1F-9D2A-7E4B6C1D8F30}";
pub const DELEGATE_CLSID: GUID = GUID {
    data1: 0x8B6B4F72,
    data2: 0x3A5E,
    data3: 0x4C1F,
    data4: [0x9D, 0x2A, 0x7E, 0x4B, 0x6C, 0x1D, 0x8F, 0x30],
};

const SHELL_KEY: &str = r"Software\Classes\*\shell\SingboardTaildrop";
const SHELL_COMMAND_KEY: &str = r"Software\Classes\*\shell\SingboardTaildrop\command";
const CLSID_KEY: &str = r"Software\Classes\CLSID\{8B6B4F72-3A5E-4C1F-9D2A-7E4B6C1D8F30}";
const LOCAL_SERVER_KEY: &str =
    r"Software\Classes\CLSID\{8B6B4F72-3A5E-4C1F-9D2A-7E4B6C1D8F30}\LocalServer32";
const PREFS_KEY: &str = r"Software\Singboard\Taildrop";
const LAST_PEER_VALUE: &str = "LastPeerId";

fn live_exe_path() -> Option<String> {
    let process = unsafe { GetCurrentProcess() };
    let mut cap = 1024usize;
    while cap <= 32_768 {
        let mut buf = vec![0u16; cap];
        let mut len = cap as u32;
        if unsafe { QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len) } != 0
            && len > 0
        {
            return Some(OsString::from_wide(&buf[..len as usize]).to_string_lossy().into_owned());
        }
        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
            break;
        }
        cap *= 2;
    }
    None
}

fn exe_path() -> Result<String, String> {
    if let Some(path) = live_exe_path() {
        return Ok(path);
    }
    let exe = env::current_exe().map_err(|e| format!("获取程序路径失败: {e}"))?;
    Ok(exe.display().to_string())
}

fn local_server_command(exe: &str) -> String {
    format!("\"{exe}\" {DELEGATE_FLAG}")
}

fn icon_value(exe: &str) -> String {
    format!("\"{exe}\",0")
}

pub fn is_registered() -> bool {
    let Ok(exe) = exe_path() else {
        return false;
    };
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let delegate = hkcu
        .open_subkey(SHELL_COMMAND_KEY)
        .and_then(|k| k.get_value::<String, _>("DelegateExecute"))
        .ok();
    let server = hkcu
        .open_subkey(LOCAL_SERVER_KEY)
        .and_then(|k| k.get_value::<String, _>(""))
        .ok();
    let expected = local_server_command(&exe);
    delegate.is_some_and(|v| v.eq_ignore_ascii_case(DELEGATE_CLSID_STR))
        && server.is_some_and(|v| v.eq_ignore_ascii_case(&expected))
}

pub fn set_registered(enabled: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if enabled {
        let exe = exe_path()?;
        let (shell, _) = hkcu
            .create_subkey(SHELL_KEY)
            .map_err(|e| format!("写入右键菜单注册表失败: {e}"))?;
        shell
            .set_value("", &"通过 Taildrop 发送")
            .map_err(|e| format!("写入右键菜单名称失败: {e}"))?;
        shell
            .set_value("Icon", &icon_value(&exe))
            .map_err(|e| format!("写入右键菜单图标失败: {e}"))?;
        shell
            .set_value("MultiSelectModel", &"Player")
            .map_err(|e| format!("写入右键菜单多选属性失败: {e}"))?;

        let (cmd, _) = hkcu
            .create_subkey(SHELL_COMMAND_KEY)
            .map_err(|e| format!("写入右键命令注册表失败: {e}"))?;
        cmd.set_value("DelegateExecute", &DELEGATE_CLSID_STR)
            .map_err(|e| format!("写入 DelegateExecute 失败: {e}"))?;

        let (server, _) = hkcu
            .create_subkey(LOCAL_SERVER_KEY)
            .map_err(|e| format!("写入 COM 服务注册表失败: {e}"))?;
        server
            .set_value("", &local_server_command(&exe))
            .map_err(|e| format!("写入 LocalServer32 失败: {e}"))?;
    } else {
        for key in [SHELL_KEY, CLSID_KEY] {
            if let Err(e) = hkcu.delete_subkey_all(key)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                return Err(format!("清理右键菜单注册表失败: {e}"));
            }
        }
    }
    Ok(())
}

pub fn repair() {
    if cfg!(debug_assertions) {
        return;
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let has_delegate = hkcu
        .open_subkey(SHELL_COMMAND_KEY)
        .and_then(|k| k.get_value::<String, _>("DelegateExecute"))
        .is_ok_and(|v| v.eq_ignore_ascii_case(DELEGATE_CLSID_STR));
    if !has_delegate {
        return;
    }
    let Ok(exe) = exe_path() else {
        return;
    };
    let expected_server = local_server_command(&exe);
    let expected_icon = icon_value(&exe);
    let current_server = hkcu
        .open_subkey(LOCAL_SERVER_KEY)
        .and_then(|k| k.get_value::<String, _>(""))
        .ok();
    let current_icon = hkcu
        .open_subkey(SHELL_KEY)
        .and_then(|k| k.get_value::<String, _>("Icon"))
        .ok();
    let server_ok = current_server.is_some_and(|v| v.eq_ignore_ascii_case(&expected_server));
    let icon_ok = current_icon.is_some_and(|v| v.eq_ignore_ascii_case(&expected_icon));
    if !server_ok || !icon_ok {
        let _ = set_registered(true);
    }
}

pub fn last_peer_id() -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(PREFS_KEY)
        .and_then(|k| k.get_value::<String, _>(LAST_PEER_VALUE))
        .ok()
        .filter(|s| !s.trim().is_empty())
}

pub fn save_last_peer_id(peer_id: &str) {
    if peer_id.trim().is_empty() {
        return;
    }
    if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(PREFS_KEY) {
        let _ = key.set_value(LAST_PEER_VALUE, &peer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_local_server_and_icon_values() {
        assert_eq!(
            local_server_command(r"C:\Tools\singboard.exe"),
            r#""C:\Tools\singboard.exe" --taildrop-delegate"#
        );
        assert_eq!(
            icon_value(r"C:\Tools\singboard.exe"),
            r#""C:\Tools\singboard.exe",0"#
        );
    }

    #[test]
    fn live_exe_path_resolves_current_binary() {
        let path = live_exe_path().expect("live exe path");
        assert!(std::path::Path::new(&path).is_file());
    }
}
