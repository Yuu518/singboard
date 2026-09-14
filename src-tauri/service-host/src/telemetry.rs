use crate::ipc;
use serde::{Deserialize, Serialize};
use std::io;
use std::time::{Duration, Instant};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::sync::oneshot;

#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub pid: u32,
    pub uptime_seconds: u64,
}

pub fn pipe_name(service_name: &str) -> String {
    use sha2::{Digest, Sha256};
    // Bound the name even for a 256-character SCM service name.
    let encoded = format!(
        "{:x}",
        Sha256::digest(service_name.to_lowercase().as_bytes())
    );
    format!(r"\\.\pipe\singboard-status-{encoded}")
}

pub struct Server {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    pub fn start(service_name: &str, sid: &str) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let pipe = {
            let _entered = runtime.enter();
            ipc::server(&pipe_name(service_name), sid, true)?
        };
        let (stop, mut stopped) = oneshot::channel();
        let started = Instant::now();
        let thread = std::thread::spawn(move || {
            runtime.block_on(async move {
            let mut pipe = pipe;
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    connected = pipe.connect() => {
                        if connected.is_err() { break; }
                        let snapshot = Snapshot { version: 1, pid: std::process::id(), uptime_seconds: started.elapsed().as_secs() };
                        let _ = tokio::time::timeout(Duration::from_millis(250), ipc::send(&mut pipe, &snapshot)).await;
                        // Wait for the reader to close, bounded so a stalled client cannot monopolize the endpoint.
                        let deadline = Instant::now() + Duration::from_millis(250);
                        while Instant::now() < deadline && ipc::client_pid(&pipe).is_ok() {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                        let _ = pipe.disconnect();
                    }
                }
            }
        })
        });
        Ok(Self {
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub async fn query(service_name: &str, expected_pid: u32) -> Option<u64> {
    tokio::time::timeout(Duration::from_millis(600), async {
        let name = pipe_name(service_name);
        let mut pipe = loop {
            match ClientOptions::new().read(true).write(false).open(&name) {
                Ok(pipe) => break pipe,
                Err(e) if e.raw_os_error() == Some(231) => {
                    tokio::time::sleep(Duration::from_millis(20)).await
                }
                Err(_) => return None,
            }
        };
        if ipc::server_pid(&pipe).ok()? != expected_pid {
            return None;
        }
        let snapshot: Snapshot = ipc::receive(&mut pipe).await.ok()?;
        (snapshot.version == 1 && snapshot.pid == expected_pid).then_some(snapshot.uptime_seconds)
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipe_names_are_case_insensitive_and_bounded() {
        assert_eq!(pipe_name("Sing-Box"), pipe_name("sing-box"));
        assert!(pipe_name(&"服".repeat(256)).len() < 256);
    }
    #[test]
    fn real_pipe_reports_uptime_and_rejects_another_process_instance() {
        let name = format!("singboard-telemetry-test-{}", std::process::id());
        let sid = ipc::current_user_sid().unwrap();
        let host = Server::start(&name, &sid).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            assert!(query(&name, std::process::id()).await.is_some());
            assert_eq!(query(&name, std::process::id() + 1).await, None);
            tokio::time::sleep(Duration::from_millis(1100)).await;
            assert!(query(&name, std::process::id()).await.unwrap() >= 1);
        });
        drop(host);
        assert_eq!(runtime.block_on(query(&name, std::process::id())), None);
    }
}
