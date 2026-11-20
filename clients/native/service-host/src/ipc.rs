//! Local, bounded IPC. Management and telemetry use different endpoints.
use serde::{Serialize, de::DeserializeOwned};
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Threading::*;

pub const MAX_FRAME: usize = 64 * 1024;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn process_token() -> io::Result<OwnedHandle> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

pub fn is_elevated() -> io::Result<bool> {
    let token = process_token()?;
    let mut info: TOKEN_ELEVATION = unsafe { std::mem::zeroed() };
    let mut size = 0;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenElevation,
            &mut info as *mut _ as _,
            std::mem::size_of_val(&info) as u32,
            &mut size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info.TokenIsElevated != 0)
}

pub fn current_user_sid() -> io::Result<String> {
    let token = process_token()?;
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut size,
        );
    }
    // usize storage gives TOKEN_USER the alignment it requires.
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr() as _,
            size,
            &mut size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
    let mut text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = unsafe {
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text as _);
        value
    };
    Ok(result)
}

pub fn valid_sid(sid: &str) -> bool {
    sid.starts_with("S-1-")
        && sid.len() < 184
        && sid
            .split('-')
            .skip(1)
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

pub fn server(name: &str, sid: &str, read_only: bool) -> io::Result<NamedPipeServer> {
    if !valid_sid(sid) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid user SID",
        ));
    }
    let sddl = wide(&format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;{sid})"));
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let result = unsafe {
        ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .access_inbound(!read_only)
            .access_outbound(true)
            .create_with_security_attributes_raw(name, &mut attributes as *mut _ as _)
    };
    unsafe {
        LocalFree(descriptor);
    }
    result
}

pub fn server_pid(pipe: &impl AsRawHandle) -> io::Result<u32> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid)
}

pub fn client_pid(pipe: &impl AsRawHandle) -> io::Result<u32> {
    let mut pid = 0;
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid)
}

pub async fn send<T: Serialize>(pipe: &mut (impl AsyncWrite + Unpin), value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::other("IPC frame too large"));
    }
    pipe.write_u32(bytes.len() as u32).await?;
    pipe.write_all(&bytes).await
}

pub async fn receive<T: DeserializeOwned>(pipe: &mut (impl AsyncRead + Unpin)) -> io::Result<T> {
    let size = pipe.read_u32().await? as usize;
    if size > MAX_FRAME {
        return Err(io::Error::other("IPC frame too large"));
    }
    let mut bytes = vec![0; size];
    pipe.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_sddl_injection() {
        assert!(valid_sid("S-1-5-21-123-456-789-1001"));
        assert!(!valid_sid("S-1-5-21)(A;;GA;;;WD)"));
        assert!(!valid_sid("S-1-"));
    }
    #[test]
    fn rejects_oversized_incoming_frames_before_allocating() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (mut writer, mut reader) = tokio::io::duplex(16);
            writer.write_u32(MAX_FRAME as u32 + 1).await.unwrap();
            assert!(receive::<serde_json::Value>(&mut reader).await.is_err());
        });
    }
}
