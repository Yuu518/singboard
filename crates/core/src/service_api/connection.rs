use super::{ApiError, Result};
use crate::settings::{Endpoint, Settings};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use tokio_util::sync::CancellationToken;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint as GrpcEndpoint};
use tonic::{Request, Status, service::Interceptor};
use url::{Host, Url};

pub(super) type Client =
    super::proto::started_service_client::StartedServiceClient<InterceptedService<Channel, Auth>>;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Connection {
    pub url: String,
    pub secret: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ca_pem: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub server_name: String,
}

#[derive(Clone)]
pub(super) struct Auth(Option<MetadataValue<Ascii>>);

impl Interceptor for Auth {
    fn call(&mut self, mut request: Request<()>) -> std::result::Result<Request<()>, Status> {
        if let Some(value) = &self.0 {
            request
                .metadata_mut()
                .insert("authorization", value.clone());
        }
        Ok(request)
    }
}

impl Connection {
    pub(super) fn transport(&self) -> Result<(GrpcEndpoint, Auth)> {
        let invalid = || ApiError::invalid("Service API 地址应为 http(s)://主机:端口");
        let url: Url = self.url.parse().map_err(|_| invalid())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(invalid());
        }
        let mut endpoint = GrpcEndpoint::from_shared(url.to_string())
            .map_err(|_| invalid())?
            .connect_timeout(super::TIMEOUT);
        if url.scheme() == "https" {
            let mut tls = ClientTlsConfig::new().with_enabled_roots();
            if !self.ca_pem.is_empty() {
                tls = tls.ca_certificate(Certificate::from_pem(&self.ca_pem));
            }
            if !self.server_name.is_empty() {
                tls = tls.domain_name(self.server_name.clone());
            }
            endpoint = endpoint.tls_config(tls).map_err(ApiError::invalid)?;
        } else if !self.ca_pem.is_empty() || !self.server_name.is_empty() {
            return Err(ApiError::invalid("TLS 设置需要 HTTPS 地址"));
        }
        let auth = if self.secret.is_empty() {
            None
        } else {
            let mut value = MetadataValue::try_from(format!("Bearer {}", self.secret))
                .map_err(|_| ApiError::invalid("Service API 密钥含有无效字符"))?;
            value.set_sensitive(true);
            Some(value)
        };
        Ok((endpoint, Auth(auth)))
    }

    pub(super) async fn connect(&self) -> Result<Client> {
        let (endpoint, auth) = self.transport()?;
        let channel = endpoint
            .connect()
            .await
            .map_err(|e| ApiError::new("unavailable", e))?;
        Ok(Client::new(InterceptedService::new(channel, auth)))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Target {
    id: String,
    connection: Option<Connection>,
    config_path: Option<PathBuf>,
}

fn local_config(settings: &Settings, endpoint: &Endpoint) -> Option<PathBuf> {
    if settings.config_path.is_empty()
        || (cfg!(target_os = "android") && !settings.native_root_enabled)
    {
        return None;
    }
    let url = Url::parse(&endpoint.url).ok()?;
    let local = match url.host()? {
        Host::Domain(host) => host.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => ip.is_loopback(),
    };
    local.then(|| PathBuf::from(&settings.config_path))
}

impl Target {
    fn active(settings: &Settings) -> Option<Self> {
        let endpoint = settings.active()?;
        Some(Self {
            id: endpoint.id.clone(),
            connection: endpoint.service_api.clone(),
            config_path: endpoint
                .service_api
                .is_none()
                .then(|| local_config(settings, endpoint))
                .flatten(),
        })
    }
}

pub(super) struct Candidate {
    pub tag: String,
    pub connection: Connection,
}

fn local_candidates(path: &Path) -> Result<Vec<Candidate>> {
    #[cfg(target_os = "android")]
    crate::root::set_enabled(true);
    #[cfg(target_os = "android")]
    let contents = crate::root::fs::read_to_string(path).map_err(ApiError::config)?;
    #[cfg(not(target_os = "android"))]
    let contents = std::fs::read_to_string(path).map_err(ApiError::config)?;
    let config = crate::profiles::parse_config(&contents).map_err(ApiError::config)?;
    let mut result = Vec::new();
    for (index, item) in config["services"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if item["type"] != "api" {
            continue;
        }
        let Some(port) = item["listen_port"]
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| *n != 0)
        else {
            continue;
        };
        let host = item["listen"].as_str().unwrap_or("127.0.0.1");
        let mut ip = host.parse::<IpAddr>().map_err(ApiError::config)?;
        if ip.is_unspecified() {
            ip = if ip.is_ipv4() {
                Ipv4Addr::LOCALHOST.into()
            } else {
                Ipv6Addr::LOCALHOST.into()
            };
        }
        let scheme = if item["tls"]["enabled"] == true {
            "https"
        } else {
            "http"
        };
        result.push(Candidate {
            tag: item["tag"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| index.to_string()),
            connection: Connection {
                url: format!("{scheme}://{}", SocketAddr::new(ip, port)),
                secret: item["secret"].as_str().unwrap_or_default().into(),
                ..Default::default()
            },
        });
    }
    Ok(result)
}

pub(super) struct Binding {
    dir: PathBuf,
    target: Target,
    pub cancel: CancellationToken,
}

static BINDINGS: Mutex<Vec<Weak<Binding>>> = Mutex::new(Vec::new());

pub(crate) fn settings_changed(dir: &Path, settings: &Settings) {
    BINDINGS.lock().unwrap().retain(|weak| {
        let Some(binding) = weak.upgrade() else {
            return false;
        };
        if binding.dir == dir && Some(&binding.target) != Target::active(settings).as_ref() {
            binding.cancel.cancel();
            return false;
        }
        true
    });
}

impl Binding {
    pub fn active(dir: &Path) -> Result<Arc<Self>> {
        let mut bindings = BINDINGS.lock().unwrap();
        let settings = Settings::load(dir).map_err(ApiError::config)?;
        let target = Target::active(&settings).ok_or_else(ApiError::missing)?;
        let binding = Arc::new(Self {
            dir: dir.into(),
            target,
            cancel: CancellationToken::new(),
        });
        bindings.retain(|weak| weak.strong_count() > 0);
        bindings.push(Arc::downgrade(&binding));
        Ok(binding)
    }

    pub fn candidates(&self) -> Result<Vec<Candidate>> {
        if let Some(connection) = &self.target.connection {
            return Ok(vec![Candidate {
                tag: String::new(),
                connection: connection.clone(),
            }]);
        }
        self.target
            .config_path
            .as_deref()
            .map(local_candidates)
            .unwrap_or_else(|| Ok(Vec::new()))
    }
}
