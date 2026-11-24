use super::connection::Client;
use super::proto::*;
use super::{ApiError, Result, endpoint, endpoint_tag, transfer};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(
    tag = "feed",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Feed {
    Status,
    Ping {
        tag: Option<String>,
        peer_ip: IpAddr,
    },
    Inbox {
        tag: Option<String>,
    },
    Upload {
        tag: Option<String>,
        peer_stable_id: String,
        paths: Vec<PathBuf>,
    },
    Download {
        tag: Option<String>,
        name: String,
        destination: PathBuf,
    },
    Ssh {
        tag: Option<String>,
        peer_stable_id: String,
        username: String,
        columns: u16,
        rows: u16,
    },
}

pub(super) struct Events {
    queue: mpsc::Sender<Value>,
    latest: Arc<Mutex<Option<Value>>>,
}

impl Events {
    pub async fn emit(&self, value: Value) -> Result<()> {
        self.queue
            .send(value)
            .await
            .map_err(|_| ApiError::new("cancelled", "订阅已关闭"))
    }

    pub fn update(&self, value: Value) {
        *self.latest.lock().unwrap() = Some(value);
    }
}

pub(super) struct Subscription {
    events: mpsc::Receiver<Value>,
    latest: Arc<Mutex<Option<Value>>>,
    input: Option<mpsc::Sender<TailscaleSshClientMessage>>,
    task: JoinHandle<()>,
}

impl Subscription {
    pub fn start(client: Client, feed: Feed, cancel: CancellationToken) -> Self {
        let (queue, events) = mpsc::channel(32);
        let latest = Arc::new(Mutex::new(None));
        let output = Events {
            queue,
            latest: latest.clone(),
        };
        let (input, rx) = if matches!(feed, Feed::Ssh { .. }) {
            let (tx, rx) = mpsc::channel(16);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let task = crate::runtime().spawn(async move {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {}
                result = run(client, feed, &output, rx) => {
                    if let Err(error) = result {
                        let _ = output.emit(json!({"error": error})).await;
                    }
                }
            }
        });
        Self {
            events,
            latest,
            input,
            task,
        }
    }

    pub fn drain(&mut self) -> Value {
        let mut events = Vec::new();
        if let Some(update) = self.latest.lock().unwrap().take() {
            events.push(update);
        }
        for _ in 0..32 {
            let Ok(event) = self.events.try_recv() else {
                break;
            };
            events.push(event);
        }
        let finished = self.events.is_closed()
            && self.events.is_empty()
            && self.latest.lock().unwrap().is_none();
        json!({"events": events, "finished": finished})
    }

    fn write(&self, message: tailscale_ssh_client_message::Message) -> Result<Value> {
        let input = self
            .input
            .as_ref()
            .ok_or_else(|| ApiError::invalid("此订阅不接受 SSH 输入"))?;
        input
            .try_send(TailscaleSshClientMessage {
                message: Some(message),
            })
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => ApiError::new("busy", "SSH 输入缓冲区已满"),
                mpsc::error::TrySendError::Closed(_) => {
                    ApiError::new("disconnected", "SSH 会话已结束")
                }
            })?;
        Ok(Value::Null)
    }

    pub fn input(&self, data: &str) -> Result<Value> {
        let data = STANDARD
            .decode(data)
            .map_err(|_| ApiError::invalid("SSH 输入需要 Base64 数据"))?;
        self.write(tailscale_ssh_client_message::Message::Input(
            TailscaleSshInput { data },
        ))
    }

    pub fn resize(&self, columns: u16, rows: u16) -> Result<Value> {
        self.write(tailscale_ssh_client_message::Message::Resize(
            TailscaleSshResize {
                columns: columns.into(),
                rows: rows.into(),
                ..Default::default()
            },
        ))
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn observe<T: Serialize>(
    mut stream: tonic::Streaming<T>,
    events: &Events,
    delay: &mut u64,
) -> Result<()> {
    while let Some(update) = stream.message().await? {
        *delay = 1;
        events.update(json!({"data": update}));
    }
    Err(ApiError::new("unavailable", "状态订阅已断开"))
}

async fn watch(client: &mut Client, tag: Option<String>, events: &Events) -> Result<()> {
    let mut delay = 1;
    loop {
        let result = match &tag {
            None => match client.subscribe_tailscale_status(()).await {
                Ok(response) => observe(response.into_inner(), events, &mut delay).await,
                Err(status) => Err(status.into()),
            },
            Some(tag) => match client
                .subscribe_taildrop_inbox(SubscribeTaildropInboxRequest {
                    endpoint_tag: tag.clone(),
                })
                .await
            {
                Ok(response) => observe(response.into_inner(), events, &mut delay).await,
                Err(status) => Err(status.into()),
            },
        };
        match result {
            Err(error) if error.code == "unavailable" => {
                events.update(json!({"reconnecting": true, "error": error}));
                sleep(Duration::from_secs(delay)).await;
                delay = (2 * delay).min(10);
            }
            other => return other,
        }
    }
}

async fn run(
    mut client: Client,
    feed: Feed,
    events: &Events,
    input: Option<mpsc::Receiver<TailscaleSshClientMessage>>,
) -> Result<()> {
    match feed {
        Feed::Status => watch(&mut client, None, events).await,
        Feed::Inbox { tag } => {
            let tag = endpoint_tag(&mut client, tag).await?;
            watch(&mut client, Some(tag), events).await
        }
        Feed::Ping { tag, peer_ip } => {
            let endpoint_tag = endpoint_tag(&mut client, tag).await?;
            let mut stream = client
                .start_tailscale_ping(TailscalePingRequest {
                    endpoint_tag,
                    peer_ip: peer_ip.to_string(),
                })
                .await?
                .into_inner();
            while let Some(sample) = stream.message().await? {
                events.emit(json!({"data": sample})).await?;
            }
            Ok(())
        }
        Feed::Upload {
            tag,
            peer_stable_id,
            paths,
        } => {
            let tag = endpoint_tag(&mut client, tag).await?;
            transfer::upload(client, tag, peer_stable_id, paths, events).await
        }
        Feed::Download {
            tag,
            name,
            destination,
        } => {
            let tag = endpoint_tag(&mut client, tag).await?;
            transfer::download(client, tag, name, destination, events).await
        }
        Feed::Ssh {
            tag,
            peer_stable_id,
            username,
            columns,
            rows,
        } => {
            let endpoint = endpoint(&mut client, tag.as_deref()).await?;
            let peer = endpoint
                .user_groups
                .iter()
                .flat_map(|group| &group.peers)
                .find(|peer| peer.stable_id == peer_stable_id)
                .ok_or_else(|| ApiError::new("notFound", "找不到 SSH 设备"))?;
            let address = peer
                .tailscale_i_ps
                .first()
                .ok_or_else(|| ApiError::invalid("设备没有 Tailscale 地址"))?;
            if peer.ssh_host_keys.is_empty() {
                return Err(ApiError::invalid("设备未公布 SSH 主机密钥"));
            }
            let start = TailscaleSshStart {
                endpoint_tag: endpoint.endpoint_tag,
                peer_address: address.clone(),
                username,
                terminal_type: "xterm-256color".into(),
                columns: columns.into(),
                rows: rows.into(),
                host_keys: peer.ssh_host_keys.clone(),
                ..Default::default()
            };
            use tokio_stream::StreamExt;
            let requests = tokio_stream::once(TailscaleSshClientMessage {
                message: Some(tailscale_ssh_client_message::Message::Start(start)),
            })
            .chain(ReceiverStream::new(
                input.ok_or_else(|| ApiError::invalid("缺少 SSH 输入通道"))?,
            ));
            let mut replies = client
                .start_tailscale_ssh_session(requests)
                .await?
                .into_inner();
            use tailscale_ssh_server_message::Message;
            while let Some(reply) = replies.message().await? {
                let value = match reply.message {
                    Some(Message::Output(out)) => json!({"output": STANDARD.encode(out.data)}),
                    Some(Message::AuthBanner(banner)) => json!({"authBanner": banner.message}),
                    Some(Message::Ready(_)) => json!({"ready": true}),
                    Some(Message::Exit(exit)) => {
                        events.emit(json!({"exit": exit})).await?;
                        return Ok(());
                    }
                    Some(Message::Error(error)) => {
                        return Err(ApiError::new("sshError", error.message));
                    }
                    None => continue,
                };
                events.emit(value).await?;
            }
            Err(ApiError::new("unavailable", "SSH 会话意外断开"))
        }
    }
}
