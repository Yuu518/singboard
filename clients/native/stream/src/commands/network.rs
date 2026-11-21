use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ResolverConfig, ResolverOpts};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::proto::rr::RecordType;
use hickory_resolver::Resolver;
use reqwest;
use serde::Serialize;
use std::net::IpAddr;
#[cfg(any(target_os = "android", test))]
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::time::{Duration, Instant};

#[cfg(any(target_os = "android", test))]
fn skip_dns_name(buf: &[u8], pos: &mut usize) -> Option<()> {
    loop {
        let len = *buf.get(*pos)?;
        if len == 0 {
            *pos += 1;
            return Some(());
        }
        if (len & 0xC0) == 0xC0 {
            if *pos + 2 > buf.len() {
                return None;
            }
            *pos += 2;
            return Some(());
        }
        if (len & 0xC0) != 0 {
            return None;
        }
        *pos += 1 + len as usize;
        if *pos > buf.len() {
            return None;
        }
    }
}

#[cfg(any(target_os = "android", test))]
fn parse_dns_ips(buf: &[u8]) -> Vec<IpAddr> {
    if buf.len() < 12 {
        return Vec::new();
    }
    let rcode = buf[3] & 0x0F;
    if rcode != 0 {
        return Vec::new();
    }
    let qdcount = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let ancount = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut pos = 12;
    for _ in 0..qdcount {
        if skip_dns_name(buf, &mut pos).is_none() || pos + 4 > buf.len() {
            return Vec::new();
        }
        pos += 4;
    }
    let mut out = Vec::new();
    for _ in 0..ancount {
        if skip_dns_name(buf, &mut pos).is_none() || pos + 10 > buf.len() {
            break;
        }
        let rtype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let rclass = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]);
        let rdlen = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlen > buf.len() {
            break;
        }
        if rclass == 1 {
            if rtype == 1 && rdlen == 4 {
                out.push(IpAddr::V4(Ipv4Addr::new(
                    buf[pos],
                    buf[pos + 1],
                    buf[pos + 2],
                    buf[pos + 3],
                )));
            } else if rtype == 28 && rdlen == 16 {
                if let Ok(octets) = <[u8; 16]>::try_from(&buf[pos..pos + 16]) {
                    out.push(IpAddr::V6(Ipv6Addr::from(octets)));
                }
            }
        }
        pos += rdlen;
    }
    out
}

#[cfg(target_os = "android")]
mod android_dns {
    use super::parse_dns_ips;
    use std::ffi::{CString, c_char, c_int};
    use std::net::{IpAddr, SocketAddr};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    type ResNqueryFn = unsafe extern "C" fn(u64, *const c_char, c_int, c_int, u32) -> c_int;
    type ResNresultFn = unsafe extern "C" fn(c_int, *mut c_int, *mut u8, usize) -> c_int;
    type ResCancelFn = unsafe extern "C" fn(c_int);

    struct Api {
        nquery: ResNqueryFn,
        nresult: ResNresultFn,
        cancel: ResCancelFn,
    }

    struct ResFd {
        fd: c_int,
        cancel: ResCancelFn,
    }

    impl ResFd {
        fn new(fd: c_int, cancel: ResCancelFn) -> Option<Self> {
            (fd >= 0).then_some(Self { fd, cancel })
        }

        fn take(&mut self) -> c_int {
            std::mem::replace(&mut self.fd, -1)
        }
    }

    impl Drop for ResFd {
        fn drop(&mut self) {
            if self.fd >= 0 {
                unsafe { (self.cancel)(self.fd) };
                self.fd = -1;
            }
        }
    }

    fn load_api() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(|| unsafe {
            let handle = libc::dlopen(c"libandroid.so".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if handle.is_null() {
                return None;
            }
            let nquery = libc::dlsym(handle, c"android_res_nquery".as_ptr());
            let nresult = libc::dlsym(handle, c"android_res_nresult".as_ptr());
            let cancel = libc::dlsym(handle, c"android_res_cancel".as_ptr());
            if nquery.is_null() || nresult.is_null() || cancel.is_null() {
                return None;
            }
            Some(Api {
                nquery: std::mem::transmute(nquery),
                nresult: std::mem::transmute(nresult),
                cancel: std::mem::transmute(cancel),
            })
        })
        .as_ref()
    }

    fn fresh_lookup(host: &str) -> Option<Vec<IpAddr>> {
        const NS_C_IN: c_int = 1;
        const NS_T_A: c_int = 1;
        const NS_T_AAAA: c_int = 28;
        const NO_CACHE_LOOKUP: u32 = 1 << 2;

        let api = load_api()?;
        let cname = CString::new(host).ok()?;
        let mut fd_a = ResFd::new(
            unsafe { (api.nquery)(0, cname.as_ptr(), NS_C_IN, NS_T_A, NO_CACHE_LOOKUP) },
            api.cancel,
        );
        let mut fd_aaaa = ResFd::new(
            unsafe { (api.nquery)(0, cname.as_ptr(), NS_C_IN, NS_T_AAAA, NO_CACHE_LOOKUP) },
            api.cancel,
        );
        if fd_a.is_none() && fd_aaaa.is_none() {
            return None;
        }
        let mut v4 = Vec::new();
        let mut v6 = Vec::new();
        let mut deadline = Instant::now() + Duration::from_secs(4);
        let mut buf = [0u8; 4096];

        while fd_a.is_some() || fd_aaaa.is_some() {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let remaining_ms = deadline
                .saturating_duration_since(now)
                .as_millis()
                .min(i32::MAX as u128) as c_int;
            let mut fds = [
                libc::pollfd {
                    fd: fd_a.as_ref().map_or(-1, |f| f.fd),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: fd_aaaa.as_ref().map_or(-1, |f| f.fd),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, remaining_ms) };
            if rc < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            if rc == 0 {
                break;
            }
            if fds[0].revents != 0 {
                if let Some(mut handle) = fd_a.take() {
                    let mut rcode: c_int = 0;
                    let len = unsafe {
                        (api.nresult)(handle.take(), &mut rcode, buf.as_mut_ptr(), buf.len())
                    };
                    if len > 0 && rcode == 0 {
                        let got = parse_dns_ips(&buf[..(len as usize).min(buf.len())]);
                        v4.extend(got.into_iter().filter(|ip| ip.is_ipv4()));
                        if !v4.is_empty() {
                            let short = Instant::now() + Duration::from_millis(150);
                            if short < deadline {
                                deadline = short;
                            }
                        }
                    }
                }
            }
            if fds[1].revents != 0 {
                if let Some(mut handle) = fd_aaaa.take() {
                    let mut rcode: c_int = 0;
                    let len = unsafe {
                        (api.nresult)(handle.take(), &mut rcode, buf.as_mut_ptr(), buf.len())
                    };
                    if len > 0 && rcode == 0 {
                        let got = parse_dns_ips(&buf[..(len as usize).min(buf.len())]);
                        v6.extend(got.into_iter().filter(|ip| ip.is_ipv6()));
                    }
                }
            }
        }
        v4.extend(v6);
        (!v4.is_empty()).then_some(v4)
    }

    pub struct AndroidFreshDns;

    impl reqwest::dns::Resolve for AndroidFreshDns {
        fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            let host = name.as_str().to_owned();
            Box::pin(async move {
                if let Ok(ip) = host.parse::<IpAddr>() {
                    let addrs: reqwest::dns::Addrs =
                        Box::new(std::iter::once(SocketAddr::new(ip, 0)));
                    return Ok(addrs);
                }
                let lookup_host = host.clone();
                if let Ok(Some(ips)) =
                    tokio::task::spawn_blocking(move || fresh_lookup(&lookup_host)).await
                {
                    let addrs: reqwest::dns::Addrs =
                        Box::new(ips.into_iter().map(|ip| SocketAddr::new(ip, 0)));
                    return Ok(addrs);
                }
                let fallback: Vec<SocketAddr> =
                    tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
                let addrs: reqwest::dns::Addrs = Box::new(fallback.into_iter());
                Ok(addrs)
            })
        }
    }
}

#[cfg(not(windows))]
const FALLBACK_DNS: [&str; 2] = ["223.5.5.5", "1.1.1.1"];

#[cfg(not(windows))]
pub(crate) fn get_system_proxy() -> Option<String> {
    None
}

#[cfg(windows)]
pub(crate) fn get_system_proxy() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::*;

    let settings = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;

    let enabled: u32 = settings.get_value("ProxyEnable").ok()?;
    if enabled == 0 {
        return None;
    }

    let server: String = settings.get_value("ProxyServer").ok()?;
    if server.is_empty() {
        return None;
    }

    let (scheme, addr) = if server.contains('=') {
        let pairs: Vec<(&str, &str)> = server
            .split(';')
            .filter_map(|entry| {
                let (k, v) = entry.trim().split_once('=')?;
                let v = v.trim();
                if v.is_empty() {
                    None
                } else {
                    Some((k.trim(), v))
                }
            })
            .collect();
        pairs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("https"))
            .or_else(|| pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case("http")))
            .or_else(|| pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case("socks")))
            .copied()
            .map(|(k, v)| {
                let default_scheme = if k.eq_ignore_ascii_case("socks") {
                    "socks5://"
                } else {
                    "http://"
                };
                (default_scheme, v.to_string())
            })?
    } else {
        ("http://", server.trim().to_string())
    };

    if addr.is_empty() {
        return None;
    }

    if addr.contains("://") {
        Some(addr)
    } else {
        Some(format!("{scheme}{addr}"))
    }
}

pub fn build_client(timeout: Option<Duration>) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60));

    if let Some(t) = timeout {
        builder = builder.timeout(t);
    }

    #[cfg(windows)]
    let builder = match get_system_proxy().and_then(|url| reqwest::Proxy::all(&url).ok()) {
        Some(proxy) => builder.proxy(proxy),
        None => builder.no_proxy(),
    };

    #[cfg(target_os = "android")]
    let builder = builder.dns_resolver(std::sync::Arc::new(android_dns::AndroidFreshDns));

    builder.build()
}

const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

fn describe_fetch_error(error: reqwest::Error) -> String {
    let error = error.without_url();
    if error.is_timeout() {
        "请求超时（30 秒）".into()
    } else if error.is_builder() {
        "地址无效".into()
    } else if error.is_connect() {
        #[cfg(windows)]
        {
            "无法连接服务器，请检查网络；如果开着系统代理，确认它指向的程序在运行".into()
        }
        #[cfg(not(windows))]
        {
            "无法连接服务器，请检查网络或代理状态".into()
        }
    } else if error.is_decode() || error.is_body() {
        "下载中断，内容不完整".into()
    } else {
        format!("下载失败：{error}")
    }
}

pub async fn fetch_url(url: String, user_agent: Option<String>) -> Result<String, String> {
    let client = build_client(Some(FETCH_TIMEOUT)).map_err(|e| e.to_string())?;
    let mut request = client.get(&url);
    if let Some(agent) = user_agent.filter(|agent| !agent.trim().is_empty()) {
        request = request.header(reqwest::header::USER_AGENT, agent);
    }
    let resp = request.send().await.map_err(describe_fetch_error)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("服务器返回 HTTP {}", status.as_u16()));
    }
    resp.text().await.map_err(describe_fetch_error)
}

pub async fn http_ping(url: String, count: u32) -> Result<f64, String> {
    let client = build_client(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;

    let mut total = 0.0;
    let mut success = 0u32;
    let mut last_error = None;

    for _ in 0..count {
        let start = Instant::now();
        match client.head(&url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success()
                    || status.is_redirection()
                    || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
                {
                    total += start.elapsed().as_secs_f64() * 1000.0;
                    success += 1;
                } else {
                    last_error = Some(format!("HTTP {}", status.as_u16()));
                }
            }
            Err(e) => {
                last_error = Some(if e.is_timeout() {
                    "timeout".to_string()
                } else if e.is_connect() {
                    "连接失败".to_string()
                } else {
                    "请求失败".to_string()
                });
            }
        }
    }

    if success == 0 {
        return Err(last_error.unwrap_or_else(|| "timeout".to_string()));
    }
    Ok(total / success as f64)
}

#[derive(Serialize)]
pub struct DnsRecord {
    record_type: String,
    name: String,
    ttl: u32,
    value: String,
    ip: Option<String>,
}

#[derive(Serialize)]
pub struct DnsQueryResult {
    source: String,
    server: Option<String>,
    records: Vec<DnsRecord>,
}

fn parse_record_type(record_type: &str) -> Result<RecordType, String> {
    let normalized = record_type.trim().to_ascii_uppercase();
    if normalized.is_empty() {
        return Err("DNS record type is empty".to_string());
    }
    RecordType::from_str(&normalized)
        .map_err(|_| format!("unsupported DNS record type: {}", normalized))
}

fn parse_dns_server(server: &str) -> Result<(IpAddr, u16), String> {
    let trimmed = server.trim();
    if trimmed.is_empty() {
        return Err("DNS server is empty".to_string());
    }

    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Ok((ip, 53));
    }

    if let Some((host, port_text)) = trimmed.rsplit_once(':') {
        let ip = host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .map_err(|_| "DNS server must be an IP address".to_string())?;
        let port = port_text
            .parse::<u16>()
            .map_err(|_| "DNS server port is invalid".to_string())?;
        return Ok((ip, port));
    }

    Err("DNS server must be an IP address".to_string())
}

fn name_server(ip: IpAddr, port: u16) -> NameServerConfig {
    let mut udp = ConnectionConfig::udp();
    udp.port = port;
    let mut tcp = ConnectionConfig::tcp();
    tcp.port = port;
    NameServerConfig::new(ip, true, vec![udp, tcp])
}

#[cfg(windows)]
fn system_resolver(
    _reported: &[String],
) -> Result<(Resolver<TokioRuntimeProvider>, Option<String>), String> {
    let resolver = hickory_resolver::TokioResolver::builder_tokio()
        .map_err(|e| e.to_string())?
        .with_options(ResolverOpts::default())
        .build()
        .map_err(|e| e.to_string())?;
    Ok((resolver, None))
}

#[cfg(not(windows))]
fn system_resolver(
    reported: &[String],
) -> Result<(Resolver<TokioRuntimeProvider>, Option<String>), String> {
    let servers: Vec<IpAddr> = reported
        .iter()
        .filter_map(|s| {
            let clean = s.trim().trim_start_matches('/');
            let without_zone = clean.split('%').next().unwrap_or(clean);
            let ip: IpAddr = without_zone.parse().ok()?;
            match ip {
                IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80 => None,
                _ => Some(ip),
            }
        })
        .collect();
    if servers.is_empty() {
        let fallback: Vec<IpAddr> = FALLBACK_DNS.iter().filter_map(|s| s.parse().ok()).collect();
        let name_servers = fallback.into_iter().map(|ip| name_server(ip, 53)).collect();
        return Ok((custom_resolver(name_servers)?, Some(FALLBACK_DNS.join(", "))));
    }
    let name_servers = servers.into_iter().map(|ip| name_server(ip, 53)).collect();
    Ok((custom_resolver(name_servers)?, None))
}

fn custom_resolver(
    name_servers: Vec<NameServerConfig>,
) -> Result<Resolver<TokioRuntimeProvider>, String> {
    let config = ResolverConfig::from_parts(None, vec![], name_servers);
    Resolver::builder_with_config(config, TokioRuntimeProvider::default())
        .with_options(ResolverOpts::default())
        .build()
        .map_err(|e| e.to_string())
}

fn display_dns_value(data: &hickory_resolver::proto::rr::RData) -> String {
    match data {
        hickory_resolver::proto::rr::RData::TXT(txt) => txt
            .txt_data
            .iter()
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>()
            .join(""),
        _ => data.to_string(),
    }
}

pub async fn lookup_dns(domain:String, record_type:String, server:Option<String>, reported:&[String])->Result<DnsQueryResult,String>{
    let domain = domain.trim().trim_end_matches('.').to_string();
    if domain.is_empty() {
        return Err("domain is empty".to_string());
    }

    let record_type = parse_record_type(&record_type)?;
    let server = server.and_then(|s| {
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    let (resolver, source, resolved_server) = if let Some(server_text) = server.as_deref() {
        let (ip, port) = parse_dns_server(server_text)?;
        (
            custom_resolver(vec![name_server(ip, port)])?,
            "custom".to_string(),
            server,
        )
    } else {
        let (resolver, fallback) = system_resolver(reported)?;
        let source = if fallback.is_some() {
            "fallback"
        } else {
            "system"
        };
        (resolver, source.to_string(), fallback)
    };

    let lookup = resolver
        .lookup(format!("{}.", domain), record_type)
        .await
        .map_err(|e| e.to_string())?;

    let records = lookup
        .answers()
        .iter()
        .filter_map(|record| {
            let data = &record.data;
            Some(DnsRecord {
                record_type: data.record_type().to_string(),
                name: record.name.to_string(),
                ttl: record.ttl,
                value: display_dns_value(data),
                ip: data.ip_addr().map(|ip| ip.to_string()),
            })
        })
        .collect::<Vec<_>>();

    Ok(DnsQueryResult {
        source,
        server: resolved_server,
        records,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dns_ips_handles_cname_a_and_aaaa() {
        let mut pkt = vec![
            0x12, 0x34, 0x81, 0x80, 0x00, 0x01, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00,
        ];
        pkt.extend_from_slice(&[3, b'a', b'p', b'i', 2, b'i', b'p', 2, b's', b'b', 0]);
        pkt.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
        pkt.extend_from_slice(&[0xC0, 0x0C, 0x00, 0x05, 0x00, 0x01, 0, 0, 0, 60, 0, 2, 0xC0, 0x0C]);
        pkt.extend_from_slice(&[0xC0, 0x0C, 0x00, 0x01, 0x00, 0x01, 0, 0, 0, 60, 0, 4, 198, 18, 0, 9]);
        pkt.extend_from_slice(&[
            0xC0, 0x0C, 0x00, 0x1C, 0x00, 0x01, 0, 0, 0, 60, 0, 16,
            0xFC, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9,
        ]);
        let ips = parse_dns_ips(&pkt);
        assert_eq!(
            ips,
            vec![
                IpAddr::V4(Ipv4Addr::new(198, 18, 0, 9)),
                IpAddr::V6(Ipv6Addr::new(0xFC00, 0, 0, 0, 0, 0, 0, 9)),
            ]
        );
    }

    #[test]
    fn parse_dns_ips_rejects_error_rcode_and_truncated_packets() {
        let mut nxdomain = vec![
            0x12, 0x34, 0x81, 0x83, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        ];
        nxdomain.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x01, 0, 0, 0, 1, 0, 4, 1, 2, 3, 4]);
        assert!(parse_dns_ips(&nxdomain).is_empty());
        assert!(parse_dns_ips(&[0x12, 0x34]).is_empty());
        assert!(parse_dns_ips(&[0, 0, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0, 0xC0]).is_empty());
    }
}
