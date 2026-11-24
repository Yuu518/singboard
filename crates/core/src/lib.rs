use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::{Error, Message};
use url::Url;

pub mod clash;
pub mod commands;
#[cfg(windows)]
pub mod config_files;
#[cfg(target_os = "android")]
mod core_actions;
pub mod dashboard;
pub mod editor;
mod ffi;
pub mod profiles;
pub mod service_api;
pub mod settings;
pub mod tools;
pub mod update;
use clash::Clash;

pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("启动网络运行时"))
}

#[allow(dead_code)]
mod root;

pub const LOG_LIMIT: usize = 5000;
const SAMPLE_LIMIT: usize = 60;

#[derive(Deserialize, Serialize)]
pub struct LogLine {
    #[serde(rename = "type")]
    pub level: String,
    pub payload: String,
    #[serde(skip_deserializing)]
    pub time: u64,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct Traffic {
    pub up: u64,
    pub down: u64,
}

#[derive(Serialize)]
pub struct Batch {
    pub status: &'static str,
    pub failures: u32,
    pub has_frame: bool,
    pub logs: VecDeque<LogLine>,
    pub traffic: VecDeque<Traffic>,
    pub dropped: usize,
    pub memory: Option<u64>,
    pub memory_open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connections: Option<Arc<Value>>,
    #[serde(skip_serializing_if = "VecDeque::is_empty")]
    pub closed: VecDeque<Value>,
}

impl Batch {
    fn pending() -> Self {
        Self {
            status: "连接中",
            failures: 0,
            has_frame: false,
            logs: VecDeque::new(),
            traffic: VecDeque::new(),
            dropped: 0,
            memory: None,
            memory_open: false,
            connections: None,
            closed: VecDeque::new(),
        }
    }

    fn disconnected(&mut self, feed: Feed, status: &'static str) {
        if matches!(feed, Feed::Memory) {
            self.memory_open = false;
            self.memory = None;
            return;
        }
        self.status = status;
        self.has_frame = false;
        self.failures = self.failures.saturating_add(1);
        match feed {
            Feed::Connections => self.connections = None,
            Feed::Traffic => self.traffic.clear(),
            _ => {}
        }
    }
}

fn normalize_log_level(level: &str) -> &'static str {
    match level.trim().to_ascii_lowercase().as_str() {
        "panic" => "panic",
        "fatal" => "fatal",
        "error" => "error",
        "warn" | "warning" => "warn",
        "debug" => "debug",
        "trace" => "trace",
        _ => "info",
    }
}

#[derive(Clone, Copy)]
pub enum Feed {
    Logs,
    Traffic,
    Connections,
    Memory,
}

pub struct Subscription {
    batch: Arc<Mutex<Batch>>,
    task: JoinHandle<()>,
}

impl Subscription {
    pub fn start(base: &str, secret: &str, feed: Feed) -> Result<Self, String> {
        if matches!(feed, Feed::Logs) {
            return Self::start_logs(base, secret, "info");
        }
        Self::start_with_level(base, secret, feed, None)
    }

    pub fn start_logs(base: &str, secret: &str, level: &str) -> Result<Self, String> {
        Self::start_with_level(base, secret, Feed::Logs, Some(normalize_log_level(level)))
    }

    fn start_with_level(
        base: &str,
        secret: &str,
        feed: Feed,
        level: Option<&str>,
    ) -> Result<Self, String> {
        let name = match feed {
            Feed::Logs => "logs",
            Feed::Traffic => "traffic",
            Feed::Connections => "connections",
            Feed::Memory => "memory",
        };
        let mut url = Clash::new(base, secret)?.socket(name)?;
        if let Some(level) = level {
            url.query_pairs_mut().append_pair("level", level);
        }
        let batch = Arc::new(Mutex::new(Batch::pending()));
        let memory_url = if matches!(feed, Feed::Connections) {
            Some(Clash::new(base, secret)?.socket("memory")?)
        } else {
            None
        };
        let state = batch.clone();
        let task = runtime().spawn(async move {
            if let Some(memory_url) = memory_url {
                tokio::join!(
                    receive(memory_url, Feed::Memory, state.clone()),
                    receive(url, feed, state)
                );
            } else {
                receive(url, feed, state).await;
            }
        });
        Ok(Self { batch, task })
    }

    pub fn drain(&self) -> Batch {
        let mut batch = self.batch.lock().unwrap();
        Batch {
            status: batch.status,
            failures: batch.failures,
            has_frame: batch.has_frame,
            logs: std::mem::take(&mut batch.logs),
            traffic: std::mem::take(&mut batch.traffic),
            dropped: std::mem::take(&mut batch.dropped),
            memory: batch.memory.take(),
            memory_open: batch.memory_open,
            connections: batch.connections.take(),
            closed: std::mem::take(&mut batch.closed),
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn receive(mut url: Url, feed: Feed, batch: Arc<Mutex<Batch>>) {
    let mut delay = 1;
    let mut previous;
    let idle_timeout = match feed {
        Feed::Logs => Duration::from_secs(45),
        _ => Duration::from_secs(15),
    };
    loop {
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async(url.as_str()),
        )
        .await;
        let status = match result {
            Ok(Ok((mut socket, _))) => {
                previous = None;
                let mut received_frame = false;
                let mut ping_sent = false;
                if !matches!(feed, Feed::Memory) {
                    let mut state = batch.lock().unwrap();
                    state.status = "已连接";
                }
                loop {
                    let wait = if ping_sent {
                        Duration::from_secs(10)
                    } else {
                        idle_timeout
                    };
                    match tokio::time::timeout(wait, socket.next()).await {
                        Ok(Some(Ok(message))) => {
                            ping_sent = false;
                            match message {
                                Message::Text(text) => {
                                    if !received_frame {
                                        received_frame = true;
                                        delay = 1;
                                        if !matches!(feed, Feed::Memory) {
                                            batch.lock().unwrap().failures = 0;
                                        }
                                    }
                                    record(&mut batch.lock().unwrap(), feed, &text, &mut previous);
                                }
                                Message::Ping(data) => {
                                    if tokio::time::timeout(
                                        Duration::from_secs(5),
                                        socket.send(Message::Pong(data)),
                                    )
                                    .await
                                    .is_err()
                                    {
                                        break;
                                    }
                                }
                                Message::Pong(_) => {}
                                Message::Close(_) => break,
                                _ => {}
                            }
                        }
                        Ok(Some(Err(_))) | Ok(None) => break,
                        Err(_) => {
                            if ping_sent {
                                break;
                            }
                            if tokio::time::timeout(
                                Duration::from_secs(5),
                                socket.send(Message::Ping(Vec::new().into())),
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                            ping_sent = true;
                        }
                    }
                }
                "连接已断开，正在重连"
            }
            Ok(Err(Error::Http(response)))
                if matches!(feed, Feed::Logs)
                    && response.status().as_u16() == 400
                    && url.query_pairs().any(|(k, v)| k == "level" && v == "trace") =>
            {
                let query = url
                    .query_pairs()
                    .filter(|(k, _)| k != "level")
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect::<Vec<_>>();
                url.set_query(None);
                url.query_pairs_mut()
                    .extend_pairs(query)
                    .append_pair("level", "debug");
                continue;
            }
            Ok(Err(Error::Http(response))) if matches!(response.status().as_u16(), 401 | 403) => {
                "认证失败，请检查密钥"
            }
            _ => "端点不可达，正在重连",
        };
        batch.lock().unwrap().disconnected(feed, status);
        tokio::time::sleep(Duration::from_secs(delay)).await;
        delay = (delay * 2).min(10);
    }
}

fn record(batch: &mut Batch, feed: Feed, text: &str, previous: &mut Option<(Arc<Value>, Instant)>) {
    match feed {
        Feed::Memory => {
            if let Ok(v) = serde_json::from_str::<Value>(text) {
                batch.memory = v["inuse"].as_u64();
                batch.memory_open = batch.memory.is_some();
            }
        }
        Feed::Logs => {
            if let Ok(mut entry) = serde_json::from_str::<LogLine>(text) {
                entry.time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if batch.logs.len() == LOG_LIMIT {
                    batch.logs.pop_front();
                    batch.dropped += 1;
                }
                batch.logs.push_back(entry);
            }
        }
        Feed::Traffic => {
            if let Ok(sample) = serde_json::from_str::<Traffic>(text) {
                if batch.traffic.len() == SAMPLE_LIMIT {
                    batch.traffic.pop_front();
                }
                batch.traffic.push_back(sample);
                batch.has_frame = true;
            }
        }
        Feed::Connections => {
            let Ok(mut snapshot) = serde_json::from_str::<Value>(text) else {
                return;
            };
            let now = Instant::now();
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if snapshot["connections"].is_null() && snapshot.get("connections").is_some() {
                snapshot["connections"] = serde_json::json!([]);
            }
            let Some(rows) = snapshot["connections"].as_array_mut() else {
                return;
            };
            let old = previous
                .as_ref()
                .and_then(|(data, time)| data["connections"].as_array().map(|rows| (rows, time)));
            let elapsed = old
                .map(|(_, time)| now.duration_since(*time).as_secs_f64())
                .unwrap_or(0.);
            let mut by_id: HashMap<_, _> = old
                .into_iter()
                .flat_map(|(rows, _)| rows)
                .filter_map(|r| Some((r["id"].as_str()?, r)))
                .collect();
            for row in rows {
                let Some(id) = row["id"].as_str() else {
                    continue;
                };
                let old = by_id.remove(id);
                for (total, speed) in [("upload", "uploadSpeed"), ("download", "downloadSpeed")] {
                    let previous_total = old.and_then(|r| r[total].as_u64());
                    let delta = row[total]
                        .as_u64()
                        .zip(previous_total)
                        .map(|(n, p)| n.saturating_sub(p))
                        .unwrap_or(0);
                    row[speed] = Value::from(if elapsed > 0. {
                        (delta as f64 / elapsed).round() as u64
                    } else {
                        0
                    });
                }
                if let Some(t) = row["start"]
                    .as_str()
                    .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                {
                    row["startMs"] = t.timestamp_millis().into();
                }
            }
            if let Some((old, _)) = old {
                for row in old
                    .iter()
                    .rev()
                    .filter(|r| r["id"].as_str().is_some_and(|id| by_id.contains_key(id)))
                {
                    let mut closed = row.clone();
                    closed["closedAt"] = stamp.into();
                    batch.closed.push_front(closed);
                }
                batch.closed.truncate(500);
            }
            snapshot["metrics"] = dashboard::metrics(&snapshot);
            let snapshot = Arc::new(snapshot);
            *previous = Some((snapshot.clone(), now));
            batch.connections = Some(snapshot);
            batch.has_frame = true;
        }
    }
}
#[cfg(target_os = "android")]
mod android_config;

#[cfg(test)]
mod connection_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rates_history_and_topology_use_consecutive_snapshots() {
        let initial = json!({"id":"a","metadata":{"network":"tcp","sourceIP":"127.0.0.1"},"rule":"Domain","rulePayload":"sample.test","chains":["exit","group"],"start":"2026-01-01T00:00:00Z","download":5,"upload":0});
        let mut b = Batch::pending();
        let mut prev = Some((
            Arc::new(json!({"connections":[initial.clone()]})),
            Instant::now() - Duration::from_secs(2),
        ));
        let mut latest = initial;
        latest["download"] = 105.into();
        record(
            &mut b,
            Feed::Connections,
            &json!({"connections":[latest]}).to_string(),
            &mut prev,
        );
        let snap = b.connections.as_ref().unwrap();
        assert!(b.has_frame);
        assert!((48..=50).contains(&snap["connections"][0]["downloadSpeed"].as_u64().unwrap()));
        assert_eq!(snap["metrics"]["tcp"], 1);
        assert_eq!(
            snap["metrics"]["flow"]["edges"].as_array().unwrap().len(),
            3
        );
        let flow = &snap["metrics"]["flow"];
        for edge in flow["edges"].as_array().unwrap() {
            assert_eq!(edge[7], 1);
            let node = &flow["nodes"][edge[5].as_u64().unwrap() as usize];
            assert_eq!(edge[0], node["layer"]);
            assert_eq!(edge[4], node["h"]);
        }
        record(
            &mut b,
            Feed::Connections,
            r#"{"connections":null}"#,
            &mut prev,
        );
        assert_eq!(b.closed.len(), 1);
        assert_eq!(b.closed[0]["id"], "a");
        record(&mut b, Feed::Memory, r#"{"inuse":9274}"#, &mut prev);
        assert_eq!(b.memory, Some(9274));
        assert!(b.memory_open);
    }
    #[test]
    fn overview_streams_invalidate_only_their_own_samples() {
        let mut traffic = Batch::pending();
        let mut connections = Batch::pending();
        let mut previous = None;
        record(&mut traffic, Feed::Traffic, r#"{"up":11,"down":27}"#, &mut None);
        record(&mut connections, Feed::Memory, r#"{"inuse":432}"#, &mut None);
        record(&mut connections, Feed::Connections, r#"{"uploadTotal":13,"downloadTotal":21,"connections":[{"id":"a","metadata":{"network":"tcp"}},{"id":"b","metadata":{"network":"udp"}},{"id":"c","metadata":{"network":"icmp"}}]}"#, &mut previous);
        let snapshot = connections.connections.as_ref().unwrap();
        let metrics = &snapshot["metrics"];
        assert_eq!(metrics["connectionCount"], 3);
        assert_eq!(metrics["tcp"], 1);
        assert_eq!(metrics["udp"], 1);
        assert_eq!(metrics["flow"]["connectionCount"], 0);
        assert_eq!(metrics["download"], 21);

        traffic.disconnected(Feed::Traffic, "连接已断开，正在重连");
        assert!(!traffic.has_frame && traffic.traffic.is_empty());
        assert!(connections.has_frame && connections.connections.is_some());
        assert_eq!(connections.memory, Some(432));
        traffic.status = "已连接";
        assert!(!traffic.has_frame);
        record(&mut traffic, Feed::Traffic, r#"{"up":0,"down":0}"#, &mut None);
        assert!(traffic.has_frame);

        connections.disconnected(Feed::Connections, "连接已断开，正在重连");
        assert!(!connections.has_frame && connections.connections.is_none());
        assert!(connections.memory_open && traffic.has_frame);
        connections.status = "已连接";
        assert!(!connections.has_frame);
        record(&mut connections, Feed::Connections, r#"{"connections":null}"#, &mut None);
        assert!(connections.has_frame);
        assert_eq!(connections.connections.as_ref().unwrap()["metrics"]["connectionCount"], 0);
        connections.disconnected(Feed::Memory, "连接已断开，正在重连");
        assert!(connections.memory.is_none() && !connections.memory_open);
        assert!(connections.has_frame && connections.connections.is_some());
        assert_eq!(connections.status, "已连接");
    }

}
