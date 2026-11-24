mod connection;
mod stream;
#[cfg(test)]
mod tests;
mod transfer;

mod proto {
    tonic::include_proto!("daemon");
}

use crate::settings::Settings;
pub use connection::Connection;
pub(crate) use connection::settings_changed;
use connection::{Binding, Client};
use proto::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Display;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
pub use stream::Feed;
use stream::Subscription;
use tokio::time::timeout;
use tonic::{Code, Status};

const TIMEOUT: Duration = Duration::from_secs(10);
type Result<T> = std::result::Result<T, ApiError>;

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    fn new(code: &'static str, message: impl Display) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }
    pub(crate) fn invalid(message: impl Display) -> Self {
        Self::new("invalidArgument", message)
    }
    fn config(message: impl Display) -> Self {
        Self::new("configuration", message)
    }
    fn missing() -> Self {
        Self::new("notConfigured", "当前后端未设置 Service API")
    }
    fn switched() -> Self {
        Self::new("backendChanged", "后端已切换，请重新连接")
    }
}

impl From<Status> for ApiError {
    fn from(status: Status) -> Self {
        let code = match status.code() {
            Code::Unauthenticated => "unauthenticated",
            Code::PermissionDenied => "permissionDenied",
            Code::Unimplemented => "unsupported",
            Code::Unavailable => "unavailable",
            Code::Unknown | Code::Internal | Code::Cancelled
                if std::error::Error::source(&status).is_some() =>
            {
                "unavailable"
            }
            Code::NotFound => "notFound",
            Code::FailedPrecondition => "failedPrecondition",
            Code::InvalidArgument => "invalidArgument",
            Code::DeadlineExceeded => "timeout",
            Code::Cancelled => "cancelled",
            _ => "rpcError",
        };
        Self::new(code, status.message())
    }
}

impl From<tokio::time::error::Elapsed> for ApiError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        Self::new("timeout", "Service API 请求超时")
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::new("io", error)
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    Configure {
        backend_id: String,
        connection: Option<Connection>,
    },
    Discover,
    Connect {
        service_tag: Option<String>,
    },
    Disconnect,
    Status,
    SetExitNode {
        tag: Option<String>,
        stable_id: String,
    },
    Logout {
        tag: Option<String>,
    },
    Certificate {
        tag: Option<String>,
        domain: String,
        #[serde(default)]
        min_validity_seconds: u32,
    },
    MarkInboxRead {
        tag: Option<String>,
    },
    DeleteFile {
        tag: Option<String>,
        name: String,
    },
    CancelReceiving {
        tag: Option<String>,
        sender_id: String,
        name: String,
    },
    Open {
        #[serde(flatten)]
        feed: Feed,
    },
    Drain {
        handle: u64,
    },
    Close {
        handle: u64,
    },
    SshInput {
        handle: u64,
        data: String,
    },
    SshResize {
        handle: u64,
        columns: u16,
        rows: u16,
    },
}

pub struct ServiceApi {
    dir: PathBuf,
    session: Option<(Arc<Binding>, Client)>,
    streams: HashMap<u64, Subscription>,
    next_handle: u64,
}

impl ServiceApi {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            session: None,
            streams: HashMap::new(),
            next_handle: 1,
        }
    }

    fn disconnect(&mut self) {
        if let Some((binding, _)) = self.session.take() {
            binding.cancel.cancel();
        }
        self.streams.clear();
    }

    pub async fn execute(&mut self, command: Command) -> Result<Value> {
        match command {
            Command::Configure {
                backend_id,
                connection,
            } => {
                if let Some(config) = &connection {
                    config.transport()?;
                }
                Settings::edit(&self.dir, |s| {
                    let endpoint = s
                        .clash_apis
                        .iter_mut()
                        .find(|endpoint| endpoint.id == backend_id)
                        .ok_or("找不到指定的后端")?;
                    endpoint.service_api = connection;
                    Ok(())
                })
                .map_err(ApiError::config)?;
                return Ok(Value::Null);
            }
            Command::Disconnect => {
                self.disconnect();
                return Ok(Value::Null);
            }
            Command::Discover => {
                let candidates = Binding::active(&self.dir)?.candidates()?;
                return Ok(json!(
                    candidates
                        .iter()
                        .map(|c| json!({"serviceTag": c.tag, "url": c.connection.url}))
                        .collect::<Vec<_>>()
                ));
            }
            Command::Connect { service_tag } => {
                self.disconnect();
                let binding = Binding::active(&self.dir)?;
                let mut candidates = binding.candidates()?;
                let candidate = if let Some(tag) = service_tag {
                    candidates
                        .into_iter()
                        .find(|c| c.tag == tag)
                        .ok_or_else(|| ApiError::new("notFound", "找不到指定的 API 服务"))?
                } else {
                    if candidates.len() > 1 {
                        return Err(ApiError::new(
                            "selectService",
                            "存在多个 API 服务，请指定 serviceTag",
                        ));
                    }
                    candidates.pop().ok_or_else(ApiError::missing)?
                };
                let (client, version) = tokio::select! {
                    biased;
                    _ = binding.cancel.cancelled() => return Err(ApiError::switched()),
                    result = async {
                        let mut client = candidate.connection.connect().await?;
                        let version = timeout(TIMEOUT, client.get_version(())).await??.into_inner();
                        Ok::<_, ApiError>((client, version))
                    } => result?,
                };
                self.session = Some((binding, client));
                return Ok(json!(version));
            }
            _ => {}
        }
        let (binding, client) = self
            .session
            .clone()
            .ok_or_else(|| ApiError::new("disconnected", "尚未连接 Service API"))?;
        if binding.cancel.is_cancelled() {
            self.disconnect();
            return Err(ApiError::switched());
        }
        tokio::select! {
            biased;
            _ = binding.cancel.cancelled() => Err(ApiError::switched()),
            result = self.connected(client, command, &binding) => result,
        }
    }

    async fn connected(
        &mut self,
        mut client: Client,
        command: Command,
        binding: &Binding,
    ) -> Result<Value> {
        match command {
            Command::Status => Ok(json!(snapshot(&mut client).await?)),
            Command::Open { feed } => {
                let handle = self.next_handle;
                self.next_handle = self
                    .next_handle
                    .checked_add(1)
                    .ok_or_else(|| ApiError::invalid("订阅句柄已用尽"))?;
                self.streams.insert(
                    handle,
                    Subscription::start(client, feed, binding.cancel.clone()),
                );
                Ok(json!(handle))
            }
            Command::Close { handle } => {
                self.streams.remove(&handle);
                Ok(Value::Null)
            }
            Command::Drain { handle } => Ok(self.stream(handle)?.drain()),
            Command::SshInput { handle, data } => self.stream(handle)?.input(&data),
            Command::SshResize {
                handle,
                columns,
                rows,
            } => self.stream(handle)?.resize(columns, rows),
            Command::SetExitNode { tag, stable_id } => {
                let endpoint_tag = endpoint_tag(&mut client, tag).await?;
                timeout(
                    TIMEOUT,
                    client.set_tailscale_exit_node(SetTailscaleExitNodeRequest {
                        endpoint_tag,
                        stable_id,
                    }),
                )
                .await??;
                Ok(Value::Null)
            }
            Command::Logout { tag } => {
                let endpoint_tag = endpoint_tag(&mut client, tag).await?;
                timeout(
                    TIMEOUT,
                    client.tailscale_logout(TailscaleLogoutRequest { endpoint_tag }),
                )
                .await??;
                Ok(Value::Null)
            }
            Command::Certificate {
                tag,
                domain,
                min_validity_seconds,
            } => {
                let endpoint = endpoint(&mut client, tag.as_deref()).await?;
                if !endpoint.cert_domains.contains(&domain) {
                    return Err(ApiError::invalid("域名不在端点的证书列表中"));
                }
                let reply = timeout(
                    Duration::from_secs(120),
                    client.get_tailscale_certificate(TailscaleCertificateRequest {
                        endpoint_tag: endpoint.endpoint_tag,
                        domain,
                        min_validity_seconds: min_validity_seconds.into(),
                    }),
                )
                .await??
                .into_inner();
                Ok(
                    json!({"certificatePem": String::from_utf8(reply.certificate_pem).map_err(ApiError::invalid)?,
                    "privateKeyPem": String::from_utf8(reply.private_key_pem).map_err(ApiError::invalid)?}),
                )
            }
            Command::MarkInboxRead { tag } => {
                let endpoint_tag = endpoint_tag(&mut client, tag).await?;
                timeout(
                    TIMEOUT,
                    client.mark_taildrop_inbox_read(MarkTaildropInboxReadRequest { endpoint_tag }),
                )
                .await??;
                Ok(Value::Null)
            }
            Command::DeleteFile { tag, name } => {
                let endpoint_tag = endpoint_tag(&mut client, tag).await?;
                timeout(
                    TIMEOUT,
                    client.delete_taildrop_file(DeleteTaildropFileRequest { endpoint_tag, name }),
                )
                .await??;
                Ok(Value::Null)
            }
            Command::CancelReceiving {
                tag,
                sender_id,
                name,
            } => {
                let endpoint_tag = endpoint_tag(&mut client, tag).await?;
                timeout(
                    TIMEOUT,
                    client.cancel_taildrop_receiving(CancelTaildropReceivingRequest {
                        endpoint_tag,
                        sender_id,
                        name,
                    }),
                )
                .await??;
                Ok(Value::Null)
            }
            _ => Err(ApiError::invalid("无效的会话操作")),
        }
    }

    fn stream(&mut self, handle: u64) -> Result<&mut Subscription> {
        self.streams
            .get_mut(&handle)
            .ok_or_else(|| ApiError::new("notFound", "订阅已关闭"))
    }
}

async fn snapshot(client: &mut Client) -> Result<TailscaleStatusUpdate> {
    timeout(TIMEOUT, async {
        client
            .subscribe_tailscale_status(())
            .await?
            .into_inner()
            .message()
            .await?
            .ok_or_else(|| Status::unavailable("未收到 Tailscale 状态"))
    })
    .await?
    .map_err(Into::into)
}

async fn endpoint(client: &mut Client, tag: Option<&str>) -> Result<TailscaleEndpointStatus> {
    let mut endpoints = snapshot(client).await?.endpoints;
    if let Some(tag) = tag {
        return endpoints
            .into_iter()
            .find(|ep| ep.endpoint_tag == tag)
            .ok_or_else(|| ApiError::new("notFound", "找不到指定的 Tailscale 端点"));
    }
    if endpoints.len() > 1 {
        return Err(ApiError::new(
            "selectEndpoint",
            "存在多个 Tailscale 端点，请指定 tag",
        ));
    }
    endpoints
        .pop()
        .ok_or_else(|| ApiError::new("noEndpoints", "核心未配置 Tailscale 端点"))
}

async fn endpoint_tag(client: &mut Client, tag: Option<String>) -> Result<String> {
    match tag {
        Some(tag) if !tag.is_empty() => Ok(tag),
        Some(_) => Err(ApiError::invalid("端点 tag 不能为空")),
        None => Ok(endpoint(client, None).await?.endpoint_tag),
    }
}
