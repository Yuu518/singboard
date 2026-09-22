use std::ffi::c_void;
use std::io::Write;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, PathBuf};
use windows_sys::Win32::Foundation::{INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath};

const DIRECTORY_SECURITY: &str = "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;BU)";
const CONFIG_SECURITY: &str = "O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x00020080;;;BU)";
const REPLACE_ACCESS: u32 =
    0x1000_0000 | 0x4000_0000 | DELETE | WRITE_DAC | WRITE_OWNER | FILE_DELETE_CHILD;
const WRITE_ACCESS: u32 =
    REPLACE_ACCESS | FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_WRITE_EA | FILE_WRITE_ATTRIBUTES;
const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

struct Descriptor(*mut c_void);

impl Descriptor {
    fn new(sddl: &str) -> Result<Self, String> {
        let mut descriptor = std::ptr::null_mut();
        let wide = singboard_service::ipc::wide(sddl);
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

unsafe fn trusted_sid(sid: PSID) -> bool {
    if sid.is_null() {
        return false;
    }
    if unsafe { IsWellKnownSid(sid, WinLocalSystemSid) != 0 }
        || unsafe { IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0 }
    {
        return true;
    }
    let mut value = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut value) } == 0 {
        return false;
    }
    let result = unsafe {
        let mut size = 0;
        while *value.add(size) != 0 {
            size += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(value, size)) == TRUSTED_INSTALLER
    };
    unsafe { LocalFree(value as _) };
    result
}

#[cfg(test)]
unsafe fn validate_descriptor(descriptor: PSECURITY_DESCRIPTOR, protected: bool) -> bool {
    unsafe { validate_descriptor_access(descriptor, protected, WRITE_ACCESS) }
}

unsafe fn validate_descriptor_access(
    descriptor: PSECURITY_DESCRIPTOR,
    protected: bool,
    writable: u32,
) -> bool {
    let mut owner = std::ptr::null_mut();
    let mut defaulted = 0;
    if unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) } == 0
        || !unsafe { trusted_sid(owner) }
    {
        return false;
    }
    if protected {
        let mut control = 0;
        let mut revision = 0;
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
            || control & SE_DACL_PROTECTED == 0
        {
            return false;
        }
    }
    let mut dacl = std::ptr::null_mut();
    let mut present = 0;
    if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
        == 0
        || present == 0
        || dacl.is_null()
    {
        return false;
    }
    for index in 0..unsafe { (*dacl).AceCount } {
        let mut ace = std::ptr::null_mut();
        if unsafe { GetAce(dacl, index as u32, &mut ace) } == 0 {
            return false;
        }
        let header = unsafe { &*(ace as *const ACE_HEADER) };
        if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 || header.AceType == 1 {
            continue;
        }
        if header.AceType != 0 {
            return false;
        }
        let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
        let sid = &allowed.SidStart as *const u32 as PSID;
        if allowed.Mask & writable != 0 && !unsafe { trusted_sid(sid) } {
            return false;
        }
    }
    true
}

fn validate_handle(handle: &OwnedHandle, protected: bool) -> Result<(), String> {
    validate_handle_access(handle, protected, WRITE_ACCESS)
}

fn validate_handle_access(
    handle: &OwnedHandle,
    protected: bool,
    writable: u32,
) -> Result<(), String> {
    let mut descriptor = std::ptr::null_mut();
    let error = unsafe {
        GetSecurityInfo(
            handle.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if error != 0 {
        return Err(std::io::Error::from_raw_os_error(error as i32).to_string());
    }
    let descriptor = Descriptor(descriptor);
    if !unsafe { validate_descriptor_access(descriptor.0, protected, writable) } {
        return Err("服务运行目录权限不安全，请使用仅管理员可写的安装目录".into());
    }
    Ok(())
}

fn open_directory(path: &Path) -> Result<OwnedHandle, String> {
    let wide = singboard_service::ipc::wide(&path.to_string_lossy());
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!(
            "无法锁定服务路径 {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut info: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
    if unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            FileAttributeTagInfo,
            &mut info as *mut _ as _,
            std::mem::size_of_val(&info) as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(format!("服务路径不能包含重解析点: {}", path.display()));
    }
    Ok(handle)
}

pub struct Directory {
    path: PathBuf,
    _handles: Vec<OwnedHandle>,
}

impl Directory {
    pub fn pin(path: &Path) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("服务路径必须为绝对路径".into());
        }
        let mut current = PathBuf::new();
        let mut handles = Vec::new();
        for component in path.components() {
            match component {
                Component::Prefix(_) => current.push(component),
                Component::RootDir | Component::Normal(_) => {
                    current.push(component);
                    handles.push(open_directory(&current)?);
                }
                _ => return Err("服务路径不能包含相对组件".into()),
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            _handles: handles,
        })
    }

    pub fn open(create: bool) -> Result<Self, String> {
        let base = program_files()?;
        let parent = Self::pin(&base)?;
        for handle in &parent._handles {
            validate_handle_access(handle, false, REPLACE_ACCESS)?;
        }
        validate_handle(parent._handles.last().ok_or("安装目录无效")?, false)?;
        let path = base.join("singboard-service");
        if create {
            let descriptor = Descriptor::new(DIRECTORY_SECURITY)?;
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: 0,
            };
            let wide = singboard_service::ipc::wide(&path.to_string_lossy());
            if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(183) {
                    return Err(format!("创建受保护服务目录失败: {error}"));
                }
            }
        }
        let handle = open_directory(&path)?;
        validate_handle(&handle, true)?;
        let mut handles = parent._handles;
        handles.push(handle);
        Ok(Self {
            path,
            _handles: handles,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_handle(self._handles.last().ok_or("服务目录无效")?, true)
    }

    pub fn validate_file(&self, path: &Path) -> Result<(), String> {
        if path.parent() != Some(self.path()) {
            return Err("服务文件不在运行目录中".into());
        }
        let wide = singboard_service::ipc::wide(&path.to_string_lossy());
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                READ_CONTROL | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut info: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
        if unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FileAttributeTagInfo,
                &mut info as *mut _ as _,
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
            || info.FileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
        {
            return Err("服务文件不能是目录或重解析点".into());
        }
        validate_handle(&handle, true)
    }

    pub fn create_child(&self, name: &str) -> Result<Self, String> {
        if !safe_name(name) {
            return Err("无效的服务运行目录名称".into());
        }
        let path = self.path.join(name);
        let descriptor = Descriptor::new(DIRECTORY_SECURITY)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let wide = singboard_service::ipc::wide(&path.to_string_lossy());
        if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Self::pin(&path)
    }

    pub fn write_new(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
        self.write_new_with_security(name, bytes, DIRECTORY_SECURITY)
    }

    pub fn write_new_config(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
        self.write_new_with_security(name, bytes, CONFIG_SECURITY)
    }

    fn write_new_with_security(
        &self,
        name: &str,
        bytes: &[u8],
        security: &str,
    ) -> Result<PathBuf, String> {
        if !safe_name(name) {
            return Err("无效的服务运行文件名称".into());
        }
        let path = self.path.join(name);
        let descriptor = Descriptor::new(security)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let wide = singboard_service::ipc::wide(&path.to_string_lossy());
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_GENERIC_WRITE,
                FILE_SHARE_READ,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut file = unsafe { std::fs::File::from_raw_handle(handle) };
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(path)
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.ends_with(['.', ' '])
        && name != "."
        && name != ".."
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '"' | '<' | '>' | '|' | '?' | '*')
        })
}

fn program_files() -> Result<PathBuf, String> {
    let mut value = std::ptr::null_mut();
    let result = unsafe {
        SHGetKnownFolderPath(&FOLDERID_ProgramFiles, 0, std::ptr::null_mut(), &mut value)
    };
    if result < 0 {
        return Err(format!("无法获取系统安装目录: {result}"));
    }
    let path = unsafe {
        let mut size = 0;
        while *value.add(size) != 0 {
            size += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(value, size));
        CoTaskMemFree(value as _);
        PathBuf::from(text)
    };
    Ok(path)
}

pub fn path() -> Result<PathBuf, String> {
    Ok(program_files()?.join("singboard-service"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_protected_administrator_owned_runtime_permissions() {
        for (sddl, expected) in [
            (DIRECTORY_SECURITY, true),
            (
                "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;BU)",
                false,
            ),
            ("O:BUG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)", false),
            ("O:BAG:BAD:(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)", false),
        ] {
            let descriptor = Descriptor::new(sddl).unwrap();
            assert_eq!(unsafe { validate_descriptor(descriptor.0, true) }, expected);
        }
    }

    #[test]
    fn ancestor_checks_allow_creation_but_reject_replacing_children() {
        let create =
            Descriptor::new("O:BAG:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x00000004;;;BU)").unwrap();
        assert!(unsafe { validate_descriptor_access(create.0, false, REPLACE_ACCESS) });
        assert!(!unsafe { validate_descriptor(create.0, false) });
        let replace =
            Descriptor::new("O:BAG:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x00000040;;;BU)").unwrap();
        assert!(!unsafe { validate_descriptor_access(replace.0, false, REPLACE_ACCESS) });
    }

    #[test]
    fn configuration_permissions_do_not_grant_other_users_content_access() {
        let descriptor = Descriptor::new(CONFIG_SECURITY).unwrap();
        assert!(unsafe { validate_descriptor(descriptor.0, true) });
        let mut dacl = std::ptr::null_mut();
        let mut present = 0;
        let mut defaulted = 0;
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted)
            },
            0
        );
        for index in 0..unsafe { (*dacl).AceCount } {
            let mut ace = std::ptr::null_mut();
            assert_ne!(unsafe { GetAce(dacl, index as u32, &mut ace) }, 0);
            let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
            let sid = &allowed.SidStart as *const u32 as PSID;
            if !unsafe { trusted_sid(sid) } {
                assert_eq!(
                    allowed.Mask & (FILE_READ_DATA | 0x8000_0000 | 0x1000_0000),
                    0
                );
            }
        }
    }

    #[test]
    fn rejects_paths_and_names_that_escape_the_selected_directory() {
        assert!(Directory::pin(Path::new("relative")).is_err());
        for name in [
            "../core.dll",
            r"child\core.dll",
            "core.dll:stream",
            ".",
            "..",
            "core.dll.",
            "core.dll ",
        ] {
            assert!(!safe_name(name));
        }
        assert!(safe_name("libcronet.dll"));
    }
}
