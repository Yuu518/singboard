use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, GetLastError};
use windows_sys::Win32::System::Services::*;

// 参数读写与错误日志逻辑已移至独立的 service-host crate,由服务宿主与面板共用
pub use singboard_service::params::{
    read_service_error_log, read_service_params, write_service_params,
};

type ScHandle = *mut std::ffi::c_void;

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn ps_single_quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn to_wide_multi(strings: &[&str]) -> Vec<u16> {
    let mut result = Vec::new();
    for s in strings {
        result.extend(OsStr::new(s).encode_wide());
        result.push(0);
    }
    result.push(0);
    result
}

fn open_scm() -> Result<ScHandle, String> {
    unsafe {
        let handle = OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_ALL_ACCESS);
        if handle.is_null() {
            Err(format!("Failed to open SCM: error {}", GetLastError()))
        } else {
            Ok(handle)
        }
    }
}

fn open_service_handle(scm: ScHandle, name: &str, access: u32) -> Result<ScHandle, String> {
    let wide_name = to_wide(name);
    unsafe {
        let handle = OpenServiceW(scm, wide_name.as_ptr(), access);
        if handle.is_null() {
            let err = GetLastError();
            if err == ERROR_SERVICE_DOES_NOT_EXIST {
                Err("service_not_found".into())
            } else {
                Err(format!("Failed to open service: error {}", err))
            }
        } else {
            Ok(handle)
        }
    }
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub state: String,
    pub pid: Option<u32>,
    pub uptime_seconds: Option<u64>,
}

// 通过进程创建时间计算运行时长（秒）
fn process_uptime_seconds(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut creation: FILETIME = std::mem::zeroed();
        let mut exit: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        // FILETIME（1601-01-01 起 100ns）转 Unix 秒
        let created_100ns =
            ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
        let created_unix = (created_100ns / 10_000_000).checked_sub(11_644_473_600)?;
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        Some(now_unix.saturating_sub(created_unix))
    }
}

pub fn query_service_status(service_name: &str) -> Result<ServiceStatus, String> {
    unsafe {
        let scm = open_scm()?;
        let svc = match open_service_handle(scm, service_name, SERVICE_QUERY_STATUS) {
            Ok(h) => h,
            Err(e) if e == "service_not_found" => {
                CloseServiceHandle(scm);
                return Ok(ServiceStatus {
                    state: "not_installed".into(),
                    pid: None,
                    uptime_seconds: None,
                });
            }
            Err(e) => {
                CloseServiceHandle(scm);
                return Err(e);
            }
        };

        let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
        let mut bytes_needed: u32 = 0;
        let ok = QueryServiceStatusEx(
            svc,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut _ as *mut u8,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut bytes_needed,
        );

        CloseServiceHandle(svc);
        CloseServiceHandle(scm);

        if ok == 0 {
            return Err(format!(
                "QueryServiceStatusEx failed: error {}",
                GetLastError()
            ));
        }

        let state = match status.dwCurrentState {
            SERVICE_RUNNING => "running",
            SERVICE_STOPPED => "stopped",
            SERVICE_START_PENDING => "starting",
            SERVICE_STOP_PENDING => "stopping",
            SERVICE_PAUSE_PENDING => "stopping",
            SERVICE_PAUSED => "stopped",
            SERVICE_CONTINUE_PENDING => "starting",
            _ => "unknown",
        };

        let pid = if status.dwProcessId != 0 {
            Some(status.dwProcessId)
        } else {
            None
        };

        Ok(ServiceStatus {
            state: state.into(),
            pid,
            uptime_seconds: if state == "running" {
                pid.and_then(process_uptime_seconds)
            } else {
                None
            },
        })
    }
}

pub fn start_service(service_name: &str) -> Result<(), String> {
    unsafe {
        let scm = open_scm()?;
        let svc = open_service_handle(
            scm,
            service_name,
            SERVICE_START | SERVICE_QUERY_STATUS | SERVICE_STOP,
        )?;

        {
            let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
            let mut bytes_needed: u32 = 0;
            let qok = QueryServiceStatusEx(
                svc,
                SC_STATUS_PROCESS_INFO,
                &mut status as *mut _ as *mut u8,
                std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut bytes_needed,
            );
            if qok != 0
                && (status.dwCurrentState == SERVICE_START_PENDING
                    || status.dwCurrentState == SERVICE_STOP_PENDING)
            {
                // Kill zombie process if it exists
                if status.dwProcessId != 0 {
                    let handle = windows_sys::Win32::System::Threading::OpenProcess(
                        windows_sys::Win32::System::Threading::PROCESS_TERMINATE,
                        0,
                        status.dwProcessId,
                    );
                    if !handle.is_null() {
                        windows_sys::Win32::System::Threading::TerminateProcess(handle, 1);
                        windows_sys::Win32::Foundation::CloseHandle(handle);
                    }
                }
                for _ in 0..20 {
                    thread::sleep(Duration::from_millis(500));
                    let qok2 = QueryServiceStatusEx(
                        svc,
                        SC_STATUS_PROCESS_INFO,
                        &mut status as *mut _ as *mut u8,
                        std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
                        &mut bytes_needed,
                    );
                    if qok2 != 0 && status.dwCurrentState == SERVICE_STOPPED {
                        break;
                    }
                }
            }
        }

        let ok = StartServiceW(svc, 0, ptr::null());

        if ok == 0 {
            let err = GetLastError();
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
            if err == 1056 {
                return Ok(());
            }
            return Err(format!("StartService failed: error {}", err));
        }

        for _ in 0..20 {
            thread::sleep(Duration::from_millis(250));
            let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
            let mut bytes_needed: u32 = 0;
            let qok = QueryServiceStatusEx(
                svc,
                SC_STATUS_PROCESS_INFO,
                &mut status as *mut _ as *mut u8,
                std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut bytes_needed,
            );
            if qok == 0 {
                continue;
            }
            match status.dwCurrentState {
                SERVICE_RUNNING => {
                    CloseServiceHandle(svc);
                    CloseServiceHandle(scm);
                    return Ok(());
                }
                SERVICE_STOPPED => {
                    CloseServiceHandle(svc);
                    CloseServiceHandle(scm);
                    let detail = read_service_error_log(service_name).unwrap_or_default();
                    let msg = if detail.is_empty() {
                        "服务启动后立即退出，可能是配置文件有误，请检查配置".to_string()
                    } else {
                        format!("服务启动失败:\n{}", detail)
                    };
                    return Err(msg);
                }
                _ => continue,
            }
        }

        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        Ok(())
    }
}

pub fn stop_service(service_name: &str) -> Result<(), String> {
    unsafe {
        let scm = open_scm()?;
        let svc = open_service_handle(scm, service_name, SERVICE_STOP | SERVICE_QUERY_STATUS)?;

        let mut status: SERVICE_STATUS = std::mem::zeroed();
        let ok = ControlService(svc, SERVICE_CONTROL_STOP, &mut status);

        if ok == 0 {
            let err = GetLastError();
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
            if err == 1062 {
                return Ok(());
            }
            return Err(format!("ControlService(STOP) failed: error {}", err));
        }

        for _ in 0..30 {
            thread::sleep(Duration::from_millis(500));
            let mut bytes_needed: u32 = 0;
            let mut proc_status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
            QueryServiceStatusEx(
                svc,
                SC_STATUS_PROCESS_INFO,
                &mut proc_status as *mut _ as *mut u8,
                std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut bytes_needed,
            );
            if proc_status.dwCurrentState == SERVICE_STOPPED {
                break;
            }
        }

        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        Ok(())
    }
}

pub fn restart_service(service_name: &str) -> Result<(), String> {
    stop_service(service_name)?;
    thread::sleep(Duration::from_millis(500));
    start_service(service_name)
}

pub fn update_service_bin_path(service_name: &str, bin_path: &str) -> Result<(), String> {
    let wide_bin = to_wide(bin_path);
    unsafe {
        let scm = open_scm()?;
        let svc = match open_service_handle(scm, service_name, SERVICE_CHANGE_CONFIG) {
            Ok(h) => h,
            Err(e) => {
                CloseServiceHandle(scm);
                return Err(e);
            }
        };

        let ok = ChangeServiceConfigW(
            svc,
            SERVICE_NO_CHANGE,
            SERVICE_NO_CHANGE,
            SERVICE_NO_CHANGE,
            wide_bin.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
        );

        CloseServiceHandle(svc);
        CloseServiceHandle(scm);

        if ok == 0 {
            return Err(format!(
                "ChangeServiceConfig failed: error {}",
                GetLastError()
            ));
        }
        Ok(())
    }
}

pub fn install_service(
    service_name: &str,
    bin_path: &str,
    display_name: &str,
) -> Result<(), String> {
    let wide_name = to_wide(service_name);
    let wide_display = to_wide(display_name);
    let wide_bin = to_wide(bin_path);
    let dependency_multi = to_wide_multi(&["Tcpip", "NlaSvc"]);

    unsafe {
        let scm = open_scm()?;

        let svc = CreateServiceW(
            scm,
            wide_name.as_ptr(),
            wide_display.as_ptr(),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_DEMAND_START,
            SERVICE_ERROR_NORMAL,
            wide_bin.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            dependency_multi.as_ptr(),
            ptr::null(),
            ptr::null(),
        );

        CloseServiceHandle(scm);

        if svc.is_null() {
            let err = GetLastError();
            if err == 1073 {
                let scm2 = open_scm()?;
                let wide_name2 = to_wide(service_name);
                let svc2 = OpenServiceW(scm2, wide_name2.as_ptr(), SERVICE_CHANGE_CONFIG);
                if !svc2.is_null() {
                    // 服务已存在时同步更新其二进制路径，确保重装能切换到新的宿主程序
                    ChangeServiceConfigW(
                        svc2,
                        SERVICE_NO_CHANGE,
                        SERVICE_DEMAND_START,
                        SERVICE_NO_CHANGE,
                        wide_bin.as_ptr(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                    );
                    CloseServiceHandle(svc2);
                }
                CloseServiceHandle(scm2);
                return Ok(());
            }
            if err == 1072 {
                CloseServiceHandle(scm);
                for _ in 0..10 {
                    thread::sleep(Duration::from_millis(500));
                    let scm2 = open_scm()?;
                    let wide_name2 = to_wide(service_name);
                    let test = OpenServiceW(scm2, wide_name2.as_ptr(), SERVICE_QUERY_STATUS);
                    if test.is_null() && GetLastError() == ERROR_SERVICE_DOES_NOT_EXIST {
                        CloseServiceHandle(scm2);
                        // Service is gone, retry install
                        return install_service(service_name, bin_path, display_name);
                    }
                    if !test.is_null() {
                        CloseServiceHandle(test);
                    }
                    CloseServiceHandle(scm2);
                }
                return Err("服务已被标记删除但尚未释放，请关闭服务管理器窗口后重试".into());
            }
            return Err(format!("CreateService failed: error {}", err));
        }

        let desc_text = to_wide("Proxy service managed by singboard");
        let mut desc = SERVICE_DESCRIPTIONW {
            lpDescription: desc_text.as_ptr() as *mut _,
        };
        ChangeServiceConfig2W(
            svc,
            SERVICE_CONFIG_DESCRIPTION,
            &mut desc as *mut _ as *mut _,
        );

        let mut actions = [
            SC_ACTION {
                Type: SC_ACTION_RESTART,
                Delay: 5000,
            },
            SC_ACTION {
                Type: SC_ACTION_RESTART,
                Delay: 10000,
            },
            SC_ACTION {
                Type: SC_ACTION_NONE,
                Delay: 0,
            },
        ];
        let mut failure = SERVICE_FAILURE_ACTIONSW {
            dwResetPeriod: 86400,
            lpRebootMsg: ptr::null_mut(),
            lpCommand: ptr::null_mut(),
            cActions: 3,
            lpsaActions: actions.as_mut_ptr(),
        };
        ChangeServiceConfig2W(
            svc,
            SERVICE_CONFIG_FAILURE_ACTIONS,
            &mut failure as *mut _ as *mut _,
        );

        CloseServiceHandle(svc);
        Ok(())
    }
}

pub fn uninstall_service(service_name: &str) -> Result<(), String> {
    let _ = stop_service(service_name);

    unsafe {
        let scm = open_scm()?;
        let svc = open_service_handle(scm, service_name, SERVICE_ALL_ACCESS)?;

        let ok = DeleteService(svc);

        CloseServiceHandle(svc);
        CloseServiceHandle(scm);

        if ok == 0 {
            let err = GetLastError();
            if err == 1072 {
                return Ok(());
            }
            return Err(format!("DeleteService failed: error {}", err));
        }
        Ok(())
    }
}

pub fn create_startup_task(service_name: &str, startup_delay_seconds: u32) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let task_name = format!("singboard-autostart-{}", service_name);
    let action_args = format!("start {}", service_name);
    let delay = startup_delay_seconds.min(3600);
    let delay_duration = format!("PT{}S", delay);
    let script = format!(
        "$u=[System.Security.Principal.WindowsIdentity]::GetCurrent().Name;\
         $a=New-ScheduledTaskAction -Execute 'sc.exe' -Argument {args};\
         $t=New-ScheduledTaskTrigger -AtLogOn;\
         $t.Delay={delay};\
         $s=New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit 0 -Compatibility Vista;\
         $s.Hidden=$true;\
         $s.DisallowStartIfOnBatteries=$false;\
         $s.StopIfGoingOnBatteries=$false;\
         $p=New-ScheduledTaskPrincipal -UserId $u -LogonType Interactive -RunLevel Highest;\
         Register-ScheduledTask -TaskName {name} -Action $a -Trigger $t -Settings $s -Principal $p -Force | Out-Null;\
         $created=Get-ScheduledTask -TaskName {name};\
         $actual=[string]$created.Triggers[0].Delay;\
         if ($actual -ne {delay}) {{ throw \"计划任务延迟保存不一致: expected {delay_text}, actual $actual\" }}",
        args = ps_single_quoted(&action_args),
        name = ps_single_quoted(&task_name),
        delay = ps_single_quoted(&delay_duration),
        delay_text = delay_duration,
    );

    let output = std::process::Command::new("powershell")
        .args(["-NonInteractive", "-NoProfile", "-Command", &script])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| format!("执行 PowerShell 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit code {:?}", output.status.code())
        };
        return Err(format!("创建任务计划失败: {}", detail));
    }
    Ok(())
}

pub fn delete_startup_task(service_name: &str) {
    use std::os::windows::process::CommandExt;

    let task_name = format!("singboard-autostart-{}", service_name);
    let _ = std::process::Command::new("schtasks")
        .args(["/Delete", "/TN", &task_name, "/F"])
        .creation_flags(0x08000000)
        .status();
}

pub fn startup_task_exists(service_name: &str) -> bool {
    use std::os::windows::process::CommandExt;

    let task_name = format!("singboard-autostart-{}", service_name);
    std::process::Command::new("schtasks")
        .args(["/Query", "/TN", &task_name])
        .creation_flags(0x08000000)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
