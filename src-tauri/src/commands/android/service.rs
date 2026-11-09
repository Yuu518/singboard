use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::root::{self, SERVICE_HOME, quote};

const SERVICE_SCRIPT: &str = include_str!("../../../scripts/service.sh");
const BOOT_SCRIPT: &str = include_str!("../../../scripts/boot.sh");
const BOOT_HOOK_DIR: &str = "/data/adb/service.d";
const BOOT_HOOK: &str = "/data/adb/service.d/singboard.sh";
const CLOCK_TICKS: f64 = 100.0;
const QUICK_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(60);
const STOP_POLL: Duration = Duration::from_millis(250);
const STOP_QUERY_RETRIES: u32 = 3;

static LAST_CPU_SAMPLE: Mutex<Option<CpuSample>> = Mutex::new(None);

#[derive(Clone, Copy, PartialEq, Debug)]
struct CpuSample {
    pid: u32,
    cpu_seconds: f64,
    uptime: f64,
}

fn script_path() -> String {
    format!("{SERVICE_HOME}/singboard.sh")
}

fn env_path() -> String {
    format!("{SERVICE_HOME}/service.env")
}

fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_bytes: Option<u64>,
}

fn with_state(state: &str) -> ServiceStatus {
    ServiceStatus {
        state: state.into(),
        ..Default::default()
    }
}

fn cpu_percent(previous: Option<CpuSample>, current: CpuSample, cpus: f64) -> Option<f64> {
    let previous = previous.filter(|p| p.pid == current.pid)?;
    let wall = current.uptime - previous.uptime;
    let used = current.cpu_seconds - previous.cpu_seconds;
    if wall <= 0.0 || used < 0.0 {
        return None;
    }
    Some((used / wall / cpus.max(1.0) * 100.0).clamp(0.0, 100.0))
}

fn parse_status(output: &str, previous: Option<CpuSample>) -> (ServiceStatus, Option<CpuSample>) {
    let fields: Vec<&str> = output.split_whitespace().collect();
    match fields.first().copied() {
        Some("running") => {
            let number = |i: usize| fields.get(i).and_then(|v| v.parse::<f64>().ok());
            let pid = fields.get(1).and_then(|v| v.parse::<u32>().ok());
            let uptime_seconds = match (number(2), number(3)) {
                (Some(start), Some(now)) if now >= start / CLOCK_TICKS => {
                    Some((now - start / CLOCK_TICKS) as u64)
                }
                _ => None,
            };
            let sample = match (pid, number(3), number(4), number(5)) {
                (Some(pid), Some(uptime), Some(utime), Some(stime)) => Some(CpuSample {
                    pid,
                    cpu_seconds: (utime + stime) / CLOCK_TICKS,
                    uptime,
                }),
                _ => None,
            };
            let cpu = sample.and_then(|s| cpu_percent(previous, s, number(7).unwrap_or(1.0)));
            let status = ServiceStatus {
                state: "running".into(),
                pid,
                uptime_seconds,
                cpu_percent: cpu,
                memory_bytes: number(6).filter(|kb| *kb > 0.0).map(|kb| (kb * 1024.0) as u64),
            };
            (status, sample)
        }
        Some("stopped") => (with_state("stopped"), None),
        Some("not_installed") => (with_state("not_installed"), None),
        _ => (with_state("unknown"), None),
    }
}

fn action_script(action: &str) -> String {
    format!(
        "[ -f {s} ] || {{ echo '服务未安装'; exit 1; }}\nsh {s} {action}",
        s = quote(&script_path())
    )
}

async fn run_action(action: &str) -> Result<String, String> {
    root::run_async(action_script(action)).await
}

pub(crate) async fn is_running() -> bool {
    matches!(query_status().await, Ok(status) if status.state == "running")
}

pub(crate) async fn query_status() -> Result<ServiceStatus, String> {
    let script = script_path();
    let output = root::run_async_within(
        format!(
            "if [ -f {s} ]; then sh {s} status; else echo not_installed; fi",
            s = quote(&script)
        ),
        QUICK_TIMEOUT,
    )
    .await?;
    let mut last = LAST_CPU_SAMPLE.lock().map_err(|_| "状态缓存异常".to_string())?;
    let (status, sample) = parse_status(&output, *last);
    *last = sample;
    Ok(status)
}

pub(crate) async fn start() -> Result<(), String> {
    run_action("start").await.map(|_| ())
}

pub(crate) async fn stop() -> Result<(), String> {
    let _ = sync_component().await;
    root::run_async_within(action_script("signal"), QUICK_TIMEOUT).await?;
    let deadline = Instant::now() + STOP_TIMEOUT;
    let mut failures = 0;
    loop {
        let failure = match query_status().await {
            Ok(status) if status.state == "running" => None,
            Ok(status) if status.state == "unknown" => Some("无法获取核心状态".to_string()),
            Ok(_) => return Ok(()),
            Err(e) => Some(e),
        };
        match failure {
            Some(e) => {
                failures += 1;
                if failures >= STOP_QUERY_RETRIES {
                    return Err(e);
                }
            }
            None => failures = 0,
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "sing-box 在 {} 秒内仍未退出；为保持网络规则一致，未强制结束",
                STOP_TIMEOUT.as_secs()
            ));
        }
        tokio::time::sleep(STOP_POLL).await;
    }
}

#[tauri::command]
pub async fn service_status() -> Result<ServiceStatus, String> {
    if !root::enabled() {
        return Ok(with_state("unknown"));
    }
    query_status().await
}

#[tauri::command]
pub async fn service_start() -> Result<(), String> {
    start().await
}

#[tauri::command]
pub async fn service_stop() -> Result<(), String> {
    stop().await
}

#[tauri::command]
pub async fn service_restart() -> Result<(), String> {
    stop().await?;
    start().await
}

fn env_content(core: &str, config: &str, working_dir: &str) -> String {
    format!(
        "CORE={}\nCONFIG={}\nWORKDIR={}\n",
        quote(core),
        quote(config),
        quote(working_dir),
    )
}

fn write_file_script(path: &str, content: &str, mode: &str) -> String {
    format!(
        "cat > {p} <<'__SINGBOARD_SCRIPT__'\n{content}__SINGBOARD_SCRIPT__\nchmod {mode} {p}\n",
        p = quote(path),
        content = if content.ends_with('\n') {
            content.to_string()
        } else {
            format!("{content}\n")
        }
    )
}

fn boot_hook_script() -> String {
    format!(
        "mkdir -p {dir}\n{write}",
        dir = quote(BOOT_HOOK_DIR),
        write = write_file_script(BOOT_HOOK, &lf(BOOT_SCRIPT), "755")
    )
}

#[tauri::command]
pub async fn service_install(
    singbox_path: String,
    config_path: String,
    working_dir: String,
) -> Result<(), String> {
    let working_dir = working_dir.trim().trim_end_matches('/');
    if working_dir.is_empty() {
        return Err("请先设置 sing-box 工作目录".into());
    }
    let core = singbox_path.trim();
    if core.is_empty() {
        return Err("未在工作目录中找到 sing-box 核心".into());
    }
    let config = config_path.trim();
    if config.is_empty() {
        return Err("未在工作目录中找到配置文件".into());
    }
    let mut script = format!(
        "set -e\nmkdir -p {home} {home}/run {home}/logs\n",
        home = quote(SERVICE_HOME)
    );
    script.push_str(&write_file_script(
        &env_path(),
        &env_content(core, config, working_dir),
        "600",
    ));
    script.push_str(&write_file_script(
        &script_path(),
        &lf(SERVICE_SCRIPT),
        "755",
    ));
    root::run_async(script).await.map(|_| ())
}

#[tauri::command]
pub async fn service_component_sync() -> Result<String, String> {
    sync_component().await
}

async fn sync_component() -> Result<String, String> {
    if !root::enabled() {
        return Ok("not_installed".into());
    }
    let script = script_path();
    let installed = root::run_async(format!(
        "[ -f {s} ] || {{ echo __missing__; exit 0; }}\ncat {s}",
        s = quote(&script)
    ))
    .await?;
    if installed.trim() == "__missing__" {
        return Ok("not_installed".into());
    }
    let expected = lf(SERVICE_SCRIPT);
    if installed.trim() == expected.trim() {
        return Ok("ok".into());
    }
    let mut update = write_file_script(&script, &expected, "755");
    update.push_str(&format!(
        "if [ -f {hook} ]; then\n{write}fi\n",
        hook = quote(BOOT_HOOK),
        write = write_file_script(BOOT_HOOK, &lf(BOOT_SCRIPT), "755")
    ));
    root::run_async(update).await?;
    Ok("updated".into())
}

#[tauri::command]
pub async fn service_error_log() -> Result<String, String> {
    run_action("log").await.or_else(|_| Ok(String::new()))
}

#[tauri::command]
pub async fn service_boot_hook_exists() -> bool {
    if !root::enabled() {
        return false;
    }
    root::run_async(format!("[ -f {} ]", quote(BOOT_HOOK)))
        .await
        .is_ok()
}

#[tauri::command]
pub async fn service_create_boot_hook() -> Result<(), String> {
    root::run_async(format!(
        "set -e\n[ -f {s} ] || {{ echo '服务未安装'; exit 1; }}\n{hook}",
        s = quote(&script_path()),
        hook = boot_hook_script()
    ))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn service_delete_boot_hook() -> Result<(), String> {
    root::run_async(format!("rm -f {}", quote(BOOT_HOOK)))
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_status_reports_uptime_memory_and_cpu_between_samples() {
        let (first, sample) = parse_status("running 1234 50000 1000.50 300 100 20480 8", None);
        assert_eq!(first.state, "running");
        assert_eq!(first.pid, Some(1234));
        assert_eq!(first.uptime_seconds, Some(500));
        assert_eq!(first.memory_bytes, Some(20480 * 1024));
        assert_eq!(first.cpu_percent, None);

        let (second, _) = parse_status("running 1234 50000 1002.50 460 100 20480 8", sample);
        let cpu = second.cpu_percent.unwrap();
        assert!((cpu - 10.0).abs() < 1e-9, "cpu was {cpu}");
    }

    #[test]
    fn cpu_is_not_reported_across_a_restarted_process() {
        let (_, sample) = parse_status("running 1 0 10 100 0 1 1", None);
        let (status, _) = parse_status("running 2 0 12 100 0 1 1", sample);
        assert_eq!(status.cpu_percent, None);
    }

    #[test]
    fn unknown_output_maps_to_unknown_state() {
        assert_eq!(parse_status("", None).0.state, "unknown");
        assert_eq!(parse_status("stopped", None).0.state, "stopped");
        assert_eq!(parse_status("not_installed", None).0.state, "not_installed");
    }

    #[test]
    fn env_keeps_core_logging_to_the_core_config() {
        let env = env_content("/w/bin/sing-box", "/w/config.json", "/w");
        assert_eq!(env, "CORE='/w/bin/sing-box'\nCONFIG='/w/config.json'\nWORKDIR='/w'\n");
    }
}
