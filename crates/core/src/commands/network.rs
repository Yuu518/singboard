use reqwest;
use serde::Serialize;
#[cfg(any(target_os = "android", test))]
use std::net::IpAddr;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::{Duration, Instant};

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

#[derive(Debug, Serialize)]
pub struct DnsRecord {
    record_type: String,
    name: String,
    ttl: u32,
    value: String,
    ip: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DnsQueryResult {
    source: String,
    server: Option<String>,
    records: Vec<DnsRecord>,
}

pub struct DohProvider {
    pub id: &'static str,
    pub label: &'static str,
    pub url: &'static str,
}

pub const DEFAULT_DOH_PROVIDER: &str = "cloudflare";

pub const DOH_PROVIDERS: [DohProvider; 4] = [
    DohProvider {
        id: "cloudflare",
        label: "Cloudflare (DoH)",
        url: "https://1.1.1.1/dns-query",
    },
    DohProvider {
        id: "google",
        label: "Google (DoH)",
        url: "https://8.8.8.8/dns-query",
    },
    DohProvider {
        id: "alidns",
        label: "AliDNS (DoH)",
        url: "https://223.5.5.5/dns-query",
    },
    DohProvider {
        id: "dnspod",
        label: "DNSPod (DoH)",
        url: "https://1.12.12.12/dns-query",
    },
];

pub fn resolve_doh_provider(server: Option<&str>) -> Result<&'static DohProvider, String> {
    let key = server.map(str::trim).filter(|s| !s.is_empty());
    let Some(key) = key else {
        return Ok(&DOH_PROVIDERS[0]);
    };
    DOH_PROVIDERS
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(key) || p.url.eq_ignore_ascii_case(key))
        .ok_or_else(|| "仅支持内置 DoH 解析器".to_string())
}

fn parse_record_type(record_type: &str) -> Result<u16, String> {
    let normalized = record_type.trim().to_ascii_uppercase();
    if normalized.is_empty() {
        return Err("DNS 记录类型不能为空".to_string());
    }
    match normalized.as_str() {
        "A" => Ok(1),
        "NS" => Ok(2),
        "CNAME" => Ok(5),
        "SOA" => Ok(6),
        "PTR" => Ok(12),
        "MX" => Ok(15),
        "TXT" => Ok(16),
        "AAAA" => Ok(28),
        "SRV" => Ok(33),
        "CAA" => Ok(257),
        _ => Err(format!("不支持的 DNS 记录类型: {}", normalized)),
    }
}

fn record_type_name(rtype: u16) -> String {
    match rtype {
        1 => "A".to_string(),
        2 => "NS".to_string(),
        5 => "CNAME".to_string(),
        6 => "SOA".to_string(),
        12 => "PTR".to_string(),
        15 => "MX".to_string(),
        16 => "TXT".to_string(),
        28 => "AAAA".to_string(),
        33 => "SRV".to_string(),
        257 => "CAA".to_string(),
        other => format!("TYPE{other}"),
    }
}

fn encode_dns_query(domain: &str, qtype: u16) -> Result<Vec<u8>, String> {
    let trimmed = domain.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return Err("请输入域名".to_string());
    }
    let normalized = match url::Host::parse(trimmed).map_err(|_| "域名格式无效".to_string())? {
        url::Host::Domain(host) => host,
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => {
            return Err("请输入域名而非 IP 地址".to_string());
        }
    };
    let mut buf = Vec::with_capacity(12 + normalized.len() + 6);
    buf.extend_from_slice(&[
        0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    let mut name_len = 1usize;
    for label in normalized.split('.') {
        let bytes = label.as_bytes();
        if bytes.is_empty() || bytes.len() > 63 {
            return Err("域名格式无效".to_string());
        }
        name_len += 1 + bytes.len();
        if name_len > 255 {
            return Err("域名长度超过限制".to_string());
        }
        buf.push(bytes.len() as u8);
        buf.extend_from_slice(bytes);
    }
    buf.push(0);
    buf.extend_from_slice(&qtype.to_be_bytes());
    buf.extend_from_slice(&1u16.to_be_bytes());
    Ok(buf)
}

fn read_dns_name(buf: &[u8], pos: &mut usize) -> Option<String> {
    let mut labels = Vec::new();
    let mut cur = *pos;
    let mut jumped = false;
    let mut steps = 0usize;
    loop {
        if steps >= 128 {
            return None;
        }
        steps += 1;
        let len = *buf.get(cur)?;
        if len == 0 {
            if !jumped {
                *pos = cur + 1;
            }
            break;
        }
        if (len & 0xC0) == 0xC0 {
            let next = *buf.get(cur + 1)?;
            let offset = (((len & 0x3F) as usize) << 8) | (next as usize);
            if offset >= buf.len() {
                return None;
            }
            if !jumped {
                *pos = cur + 2;
                jumped = true;
            }
            cur = offset;
            continue;
        }
        if (len & 0xC0) != 0 {
            return None;
        }
        let start = cur + 1;
        let end = start + len as usize;
        let slice = buf.get(start..end)?;
        labels.push(String::from_utf8_lossy(slice).into_owned());
        cur = end;
    }
    if labels.is_empty() {
        Some(".".to_string())
    } else {
        Some(format!("{}.", labels.join(".")))
    }
}

fn parse_rdata(buf: &[u8], rtype: u16, start: usize, len: usize) -> Option<(String, Option<String>)> {
    let end = start.checked_add(len)?;
    let data = buf.get(start..end)?;
    match rtype {
        1 if len == 4 => {
            let ip = Ipv4Addr::new(data[0], data[1], data[2], data[3]).to_string();
            Some((ip.clone(), Some(ip)))
        }
        28 if len == 16 => {
            let octets = <[u8; 16]>::try_from(data).ok()?;
            let ip = Ipv6Addr::from(octets).to_string();
            Some((ip.clone(), Some(ip)))
        }
        2 | 5 | 12 => {
            let mut rpos = start;
            let name = read_dns_name(buf, &mut rpos)?;
            Some((name, None))
        }
        15 if len >= 3 => {
            let pref = u16::from_be_bytes([data[0], data[1]]);
            let mut rpos = start + 2;
            let exchange = read_dns_name(buf, &mut rpos)?;
            Some((format!("{pref} {exchange}"), None))
        }
        16 => {
            let mut out = String::new();
            let mut rpos = start;
            while rpos < end {
                let slen = *buf.get(rpos)? as usize;
                rpos += 1;
                if rpos + slen > end {
                    return None;
                }
                out.push_str(&String::from_utf8_lossy(&buf[rpos..rpos + slen]));
                rpos += slen;
            }
            Some((out, None))
        }
        6 => {
            let mut rpos = start;
            let mname = read_dns_name(buf, &mut rpos)?;
            let rname = read_dns_name(buf, &mut rpos)?;
            if rpos + 20 > end {
                return None;
            }
            let serial = u32::from_be_bytes(buf[rpos..rpos + 4].try_into().ok()?);
            let refresh = u32::from_be_bytes(buf[rpos + 4..rpos + 8].try_into().ok()?);
            let retry = u32::from_be_bytes(buf[rpos + 8..rpos + 12].try_into().ok()?);
            let expire = u32::from_be_bytes(buf[rpos + 12..rpos + 16].try_into().ok()?);
            let minimum = u32::from_be_bytes(buf[rpos + 16..rpos + 20].try_into().ok()?);
            Some((
                format!("{mname} {rname} {serial} {refresh} {retry} {expire} {minimum}"),
                None,
            ))
        }
        _ => {
            let mut hex = String::with_capacity(len * 2);
            for b in data {
                hex.push_str(&format!("{b:02x}"));
            }
            Some((hex, None))
        }
    }
}

fn parse_dns_response(buf: &[u8]) -> Result<Vec<DnsRecord>, String> {
    if buf.len() < 12 || (buf[2] & 0x80) == 0 {
        return Err("DoH 响应报文无效".to_string());
    }
    let rcode = buf[3] & 0x0F;
    match rcode {
        0 => {}
        1 => return Err("DNS 查询格式错误 (FORMERR)".to_string()),
        2 => return Err("DoH 服务器解析失败 (SERVFAIL)".to_string()),
        3 => return Err("域名不存在 (NXDOMAIN)".to_string()),
        4 => return Err("DoH 服务器不支持该查询 (NOTIMP)".to_string()),
        5 => return Err("DoH 服务器拒绝查询 (REFUSED)".to_string()),
        other => return Err(format!("DNS 查询失败 (RCODE {other})")),
    }
    let qdcount = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let ancount = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut pos = 12;
    for _ in 0..qdcount {
        if skip_dns_name(buf, &mut pos).is_none() || pos + 4 > buf.len() {
            return Err("DoH 响应报文格式错误".to_string());
        }
        pos += 4;
    }
    let mut records = Vec::with_capacity(ancount);
    for _ in 0..ancount {
        let name = read_dns_name(buf, &mut pos).ok_or_else(|| "DoH 响应报文格式错误".to_string())?;
        if pos + 10 > buf.len() {
            return Err("DoH 响应报文截断".to_string());
        }
        let rtype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let rclass = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]);
        let ttl = u32::from_be_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]);
        let rdlen = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlen > buf.len() {
            return Err("DoH 响应报文截断".to_string());
        }
        if rclass == 1 {
            let (value, ip) = parse_rdata(buf, rtype, pos, rdlen)
                .ok_or_else(|| "DoH 响应记录解析失败".to_string())?;
            records.push(DnsRecord {
                record_type: record_type_name(rtype),
                name,
                ttl,
                value,
                ip,
            });
        }
        pos += rdlen;
    }
    Ok(records)
}

async fn lookup_dns_via_doh(
    domain: &str,
    record_type: &str,
    url: &str,
    client: &reqwest::Client,
) -> Result<DnsQueryResult, String> {
    let qtype = parse_record_type(record_type)?;
    let wire_query = encode_dns_query(domain, qtype)?;
    let resp = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/dns-message")
        .header(reqwest::header::ACCEPT, "application/dns-message")
        .body(wire_query)
        .send()
        .await
        .map_err(|e| {
            let e = e.without_url();
            if e.is_timeout() {
                "DoH 查询超时".to_string()
            } else if e.is_connect() {
                "无法连接 DoH 服务器，请检查网络或代理状态".to_string()
            } else {
                format!("DoH 请求失败: {e}")
            }
        })?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("DoH 服务器返回 HTTP {}", status.as_u16()));
    }
    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("读取 DoH 响应失败: {}", e.without_url()))?;
    let records = parse_dns_response(&body)?;
    Ok(DnsQueryResult {
        source: "doh".to_string(),
        server: Some(url.to_string()),
        records,
    })
}

pub async fn lookup_dns(
    domain: String,
    record_type: String,
    server: Option<String>,
) -> Result<DnsQueryResult, String> {
    let provider = resolve_doh_provider(server.as_deref())?;
    let client = build_client(Some(Duration::from_secs(10))).map_err(|e| e.to_string())?;
    lookup_dns_via_doh(&domain, &record_type, provider.url, &client).await
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

    #[test]
    fn resolves_builtin_doh_providers_and_rejects_custom_or_system() {
        assert_eq!(resolve_doh_provider(None).unwrap().id, "cloudflare");
        assert_eq!(resolve_doh_provider(Some("")).unwrap().id, "cloudflare");
        assert_eq!(resolve_doh_provider(Some("google")).unwrap().url, "https://8.8.8.8/dns-query");
        assert_eq!(resolve_doh_provider(Some("alidns")).unwrap().url, "https://223.5.5.5/dns-query");
        assert_eq!(resolve_doh_provider(Some("dnspod")).unwrap().url, "https://1.12.12.12/dns-query");
        assert_eq!(
            resolve_doh_provider(Some("https://1.1.1.1/dns-query")).unwrap().id,
            "cloudflare"
        );
        assert!(resolve_doh_provider(Some("system")).is_err());
        assert!(resolve_doh_provider(Some("custom")).is_err());
        assert!(resolve_doh_provider(Some("8.8.8.8")).is_err());
        assert!(resolve_doh_provider(Some("https://evil.example/dns-query")).is_err());
    }

    #[test]
    fn encodes_and_parses_doh_messages() {
        let query = encode_dns_query("Example.COM.", 1).unwrap();
        assert_eq!(&query[..12], &[0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            &query[12..],
            &[7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0, 0, 1, 0, 1]
        );
        assert!(encode_dns_query("", 1).is_err());
        assert!(encode_dns_query("1.1.1.1", 1).is_err());

        let mut resp = vec![
            0x00, 0x00, 0x81, 0x80, 0x00, 0x01, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00,
        ];
        resp.extend_from_slice(&[3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]);
        resp.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
        resp.extend_from_slice(&[0xC0, 0x0C, 0x00, 0x05, 0x00, 0x01, 0, 0, 0, 120, 0, 2, 0xC0, 0x10]);
        resp.extend_from_slice(&[0xC0, 0x10, 0x00, 0x01, 0x00, 0x01, 0, 0, 0, 60, 0, 4, 93, 184, 216, 34]);
        resp.extend_from_slice(&[
            0xC0, 0x10, 0x00, 0x1C, 0x00, 0x01, 0, 0, 0, 60, 0, 16,
            0x26, 0x06, 0x28, 0x00, 0x02, 0x20, 0x00, 0x01, 0x02, 0x48, 0x18, 0x93, 0x25, 0xc8, 0x19, 0x46,
        ]);

        let records = parse_dns_response(&resp).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].record_type, "CNAME");
        assert_eq!(records[0].name, "www.example.com.");
        assert_eq!(records[0].value, "example.com.");
        assert_eq!(records[0].ttl, 120);
        assert_eq!(records[0].ip, None);

        assert_eq!(records[1].record_type, "A");
        assert_eq!(records[1].name, "example.com.");
        assert_eq!(records[1].value, "93.184.216.34");
        assert_eq!(records[1].ip.as_deref(), Some("93.184.216.34"));

        assert_eq!(records[2].record_type, "AAAA");
        assert_eq!(records[2].ip.as_deref(), Some("2606:2800:220:1:248:1893:25c8:1946"));

        let nxdomain = [0, 0, 0x81, 0x83, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(parse_dns_response(&nxdomain).unwrap_err().contains("NXDOMAIN"));
    }

    #[test]
    fn performs_doh_post_and_decodes_answer() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        crate::runtime().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut req = vec![0u8; 2048];
                let n = stream.read(&mut req).await.unwrap();
                let text = String::from_utf8_lossy(&req[..n]);
                assert!(text.starts_with("POST /dns-query HTTP/1.1"));
                let header_end = req[..n].windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                let query = &req[header_end..n];
                let mut answer = query.to_vec();
                answer[2..12].copy_from_slice(&[0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
                answer.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 192, 0, 2, 42]);
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/dns-message\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    answer.len()
                );
                stream.write_all(headers.as_bytes()).await.unwrap();
                stream.write_all(&answer).await.unwrap();
            });
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let url = format!("http://{addr}/dns-query");
            let res = lookup_dns_via_doh("sample.test", "A", &url, &client)
                .await
                .unwrap();
            server.await.unwrap();
            assert_eq!(res.source, "doh");
            assert_eq!(res.server.as_deref(), Some(url.as_str()));
            assert_eq!(res.records.len(), 1);
            assert_eq!(res.records[0].value, "192.0.2.42");
            assert_eq!(res.records[0].ttl, 60);
        });
    }

    #[test]
    #[ignore]
    fn live_doh_providers_resolve_google() {
        crate::runtime().block_on(async {
            for provider in DOH_PROVIDERS {
                let res = lookup_dns(
                    "google.com".to_string(),
                    "A".to_string(),
                    Some(provider.id.to_string()),
                )
                .await
                .unwrap_or_else(|e| panic!("{} ({}) failed: {e}", provider.id, provider.url));
                assert!(
                    !res.records.is_empty(),
                    "{} ({}) returned empty records",
                    provider.id,
                    provider.url
                );
            }
        });
    }
}
