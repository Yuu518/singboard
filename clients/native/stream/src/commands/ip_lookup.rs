use crate::commands::network;
use reqwest::header::{ACCEPT, CACHE_CONTROL, USER_AGENT};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const USER_AGENT_VALUE: &str = concat!("singboard/", env!("CARGO_PKG_VERSION"));
const GEOIP_URL: &str = "https://api.ip.sb/geoip";
const IP_URL: &str = "https://api.ip.sb/ip";
const IPV4_URL: &str = "https://api-ipv4.ip.sb/ip";
const IPV6_URL: &str = "https://api-ipv6.ip.sb/ip";
const IPIP_JSON_URL: &str = "https://myip.ipip.net/json";
const IPIP_TEXT_URL: &str = "https://myip.ipip.net";
const IPIP_IP_URL: &str = "https://myip.ipip.net/ip";

const GEO_TIMEOUT: Duration = Duration::from_secs(10);
const STACK_PROBE_TIMEOUT: Duration = Duration::from_secs(6);
const CACHE_TTL: Duration = Duration::from_secs(600);
const CACHE_MAX_ENTRIES: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectIp {
    pub ip: String,
    pub location: Vec<String>,
}

impl DirectIp {
    pub fn exit_line(&self) -> String {
        let mut parts = vec![self.ip.clone()];
        parts.extend(self.location.iter().cloned());
        parts.join(" ")
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct GeoIp {
    pub ip: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continent_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asn: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asn_organization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    code: Option<u32>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct IpipJsonResponse {
    ret: Option<String>,
    data: Option<IpipJsonData>,
}

#[derive(Deserialize)]
struct IpipJsonData {
    ip: String,
    #[serde(default)]
    location: Vec<String>,
}

#[derive(Deserialize)]
struct IpipIpResponse {
    ip: String,
}

enum FetchError {
    Network(String),
    Http { status: StatusCode, message: String },
    Parse(String),
}

impl FetchError {
    fn should_fallback(&self) -> bool {
        match self {
            Self::Parse(_) => true,
            Self::Http { status, .. } => {
                !matches!(*status, StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS)
            }
            Self::Network(_) => false,
        }
    }

    fn into_message(self) -> String {
        match self {
            Self::Network(msg) | Self::Http { message: msg, .. } | Self::Parse(msg) => msg,
        }
    }
}

struct CacheEntry {
    geo: GeoIp,
    cached_at: Instant,
}

fn cache() -> &'static Mutex<HashMap<IpAddr, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<IpAddr, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

fn push_unique(parts: &mut Vec<String>, value: Option<&str>) {
    let Some(text) = non_empty(value) else {
        return;
    };
    if !parts.iter().any(|existing| existing.eq_ignore_ascii_case(text)) {
        parts.push(text.to_owned());
    }
}

impl GeoIp {
    pub fn provider(&self) -> Option<&str> {
        non_empty(self.organization.as_deref())
            .or_else(|| non_empty(self.isp.as_deref()))
            .or_else(|| non_empty(self.asn_organization.as_deref()))
    }

    pub fn overview_location(&self) -> Vec<String> {
        let mut parts = Vec::new();
        push_unique(
            &mut parts,
            non_empty(self.country.as_deref())
                .or_else(|| non_empty(self.country_code.as_deref())),
        );
        push_unique(
            &mut parts,
            non_empty(self.city.as_deref()).or_else(|| non_empty(self.region.as_deref())),
        );
        push_unique(&mut parts, self.provider());
        parts
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        push_unique(&mut parts, self.country.as_deref());
        push_unique(&mut parts, self.region.as_deref());
        push_unique(&mut parts, self.city.as_deref());
        push_unique(&mut parts, self.provider());
        push_unique(&mut parts, self.asn_organization.as_deref());
        parts.join(" ")
    }

    pub fn exit_line(&self) -> String {
        let mut parts = vec![self.ip.clone()];
        push_unique(&mut parts, self.country.as_deref());
        push_unique(&mut parts, self.region.as_deref());
        push_unique(&mut parts, self.city.as_deref());
        push_unique(&mut parts, self.provider());
        if let Some(asn) = self.asn {
            parts.push(format!("AS{asn}"));
        }
        parts.join(" ")
    }
}

fn describe_request_error(error: reqwest::Error, timeout: Duration) -> String {
    let error = error.without_url();
    if error.is_timeout() {
        format!("请求超时（{} 秒）", timeout.as_secs())
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
        "响应中断，内容不完整".into()
    } else {
        format!("请求失败：{error}")
    }
}

fn describe_http_error(status: StatusCode, body: &str, service: &str) -> String {
    let parsed = serde_json::from_str::<ApiErrorBody>(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|e| non_empty(e.message.as_deref()));
    match status {
        StatusCode::BAD_REQUEST => {
            if parsed.as_ref().and_then(|e| e.code) == Some(401) {
                "无效的 IP 地址".into()
            } else if let Some(msg) = message {
                format!("请求参数无效：{msg}")
            } else {
                "无效的 IP 地址（HTTP 400）".into()
            }
        }
        StatusCode::FORBIDDEN => format!("请求被 {service} 拒绝（HTTP 403）"),
        StatusCode::TOO_MANY_REQUESTS => "请求过于频繁，请稍后再试（HTTP 429）".into(),
        _ => match message {
            Some(msg) => format!("服务器返回 HTTP {}：{msg}", status.as_u16()),
            None => format!("服务器返回 HTTP {}", status.as_u16()),
        },
    }
}

async fn request_endpoint(
    url: &str,
    accept_json: bool,
    timeout: Duration,
    service: &str,
) -> Result<String, FetchError> {
    let client =
        network::build_client(Some(timeout)).map_err(|e| FetchError::Network(e.to_string()))?;
    let mut req = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(CACHE_CONTROL, "no-cache");
    if accept_json {
        req = req.header(ACCEPT, "application/json");
    }
    let resp = req
        .send()
        .await
        .map_err(|e| FetchError::Network(describe_request_error(e, timeout)))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| FetchError::Network(describe_request_error(e, timeout)))?;
    if !status.is_success() {
        return Err(FetchError::Http {
            status,
            message: describe_http_error(status, &body, service),
        });
    }
    Ok(body)
}

pub fn parse_ipipnet(text: &str) -> (String, Vec<String>) {
    let ip = text
        .find("IP")
        .and_then(|i| {
            let rest = &text[i + 2..];
            let rest = rest.strip_prefix('：').or_else(|| rest.strip_prefix(':'))?;
            rest.split_whitespace().next()
        })
        .and_then(|raw| raw.parse::<IpAddr>().ok())
        .map(|addr| addr.to_string())
        .unwrap_or_default();
    let mut location = Vec::new();
    if let Some(i) = text.find("来自于") {
        let rest = &text[i + "来自于".len()..];
        if let Some(rest) = rest.strip_prefix('：').or_else(|| rest.strip_prefix(':')) {
            for part in rest.split_whitespace() {
                push_unique(&mut location, Some(part));
            }
        }
    }
    (ip, location)
}

fn parse_ipip_json(body: &str) -> Result<DirectIp, String> {
    let parsed: IpipJsonResponse =
        serde_json::from_str(body).map_err(|_| "返回内容无法识别".to_owned())?;
    if !parsed
        .ret
        .as_deref()
        .is_some_and(|r| r.trim().eq_ignore_ascii_case("ok"))
    {
        return Err("返回状态异常".to_owned());
    }
    let data = parsed.data.ok_or_else(|| "返回内容缺少数据".to_owned())?;
    let ip: IpAddr = data
        .ip
        .trim()
        .parse()
        .map_err(|_| "返回内容里没有有效 IP".to_owned())?;
    let mut location = Vec::new();
    for item in &data.location {
        push_unique(&mut location, Some(item.as_str()));
    }
    Ok(DirectIp {
        ip: ip.to_string(),
        location,
    })
}

fn parse_ipip_text(body: &str) -> Result<DirectIp, String> {
    let (ip, location) = parse_ipipnet(body);
    if ip.is_empty() {
        return Err("返回内容里没有有效 IP".to_owned());
    }
    Ok(DirectIp { ip, location })
}

fn parse_ipip_ip_json(body: &str) -> Result<DirectIp, String> {
    let parsed: IpipIpResponse =
        serde_json::from_str(body).map_err(|_| "返回内容无法识别".to_owned())?;
    let ip: IpAddr = parsed
        .ip
        .trim()
        .parse()
        .map_err(|_| "返回内容里没有有效 IP".to_owned())?;
    Ok(DirectIp {
        ip: ip.to_string(),
        location: Vec::new(),
    })
}

fn parse_geoip_body(body: &str) -> Result<GeoIp, String> {
    let mut geo: GeoIp = serde_json::from_str(body).map_err(|_| "返回内容无法识别".to_owned())?;
    let parsed: IpAddr = geo
        .ip
        .trim()
        .parse()
        .map_err(|_| "返回内容里没有有效 IP".to_owned())?;
    geo.ip = parsed.to_string();
    Ok(geo)
}

fn parse_plain_ip(body: &str) -> Result<IpAddr, String> {
    body.trim()
        .parse::<IpAddr>()
        .map_err(|_| "返回内容里没有有效 IP".to_owned())
}

fn cache_get(map: &mut HashMap<IpAddr, CacheEntry>, ip: IpAddr, now: Instant) -> Option<GeoIp> {
    if let Some(entry) = map.get(&ip) {
        if now.saturating_duration_since(entry.cached_at) < CACHE_TTL {
            return Some(entry.geo.clone());
        }
        map.remove(&ip);
    }
    None
}

fn cache_put(map: &mut HashMap<IpAddr, CacheEntry>, ip: IpAddr, geo: GeoIp, now: Instant) {
    map.retain(|_, entry| now.saturating_duration_since(entry.cached_at) < CACHE_TTL);
    if map.len() >= CACHE_MAX_ENTRIES && !map.contains_key(&ip) {
        if let Some(oldest_key) = map
            .iter()
            .min_by_key(|(_, entry)| entry.cached_at)
            .map(|(k, _)| *k)
        {
            map.remove(&oldest_key);
        }
    }
    map.insert(ip, CacheEntry { geo, cached_at: now });
}

async fn fetch_direct_json_inner() -> Result<DirectIp, FetchError> {
    let body = request_endpoint(IPIP_JSON_URL, true, GEO_TIMEOUT, "ipip.net").await?;
    parse_ipip_json(&body).map_err(FetchError::Parse)
}

async fn fetch_direct_text_inner() -> Result<DirectIp, FetchError> {
    let body = request_endpoint(IPIP_TEXT_URL, false, STACK_PROBE_TIMEOUT, "ipip.net").await?;
    parse_ipip_text(&body).map_err(FetchError::Parse)
}

async fn fetch_direct_ip_only_inner() -> Result<DirectIp, FetchError> {
    let body = request_endpoint(IPIP_IP_URL, true, STACK_PROBE_TIMEOUT, "ipip.net").await?;
    parse_ipip_ip_json(&body).map_err(FetchError::Parse)
}

pub async fn fetch_direct_ip() -> Result<DirectIp, String> {
    match fetch_direct_json_inner().await {
        Ok(info) => Ok(info),
        Err(json_err) if json_err.should_fallback() => match fetch_direct_text_inner().await {
            Ok(info) => Ok(info),
            Err(text_err) if text_err.should_fallback() => {
                fetch_direct_ip_only_inner()
                    .await
                    .map_err(|_| json_err.into_message())
            }
            Err(_) => Err(json_err.into_message()),
        },
        Err(err) => Err(err.into_message()),
    }
}

async fn fetch_exit_geo_inner() -> Result<GeoIp, FetchError> {
    let body = request_endpoint(GEOIP_URL, true, GEO_TIMEOUT, "ip.sb").await?;
    parse_geoip_body(&body).map_err(FetchError::Parse)
}

pub async fn fetch_exit_geo() -> Result<GeoIp, String> {
    fetch_exit_geo_inner().await.map_err(FetchError::into_message)
}

pub async fn fetch_exit_ip() -> Result<IpAddr, String> {
    let body = request_endpoint(IP_URL, false, STACK_PROBE_TIMEOUT, "ip.sb")
        .await
        .map_err(FetchError::into_message)?;
    parse_plain_ip(&body)
}

pub async fn fetch_exit_geo_with_fallback() -> Result<GeoIp, String> {
    match fetch_exit_geo_inner().await {
        Ok(geo) => Ok(geo),
        Err(err) if err.should_fallback() => match fetch_exit_ip().await {
            Ok(ip) => Ok(GeoIp {
                ip: ip.to_string(),
                ..Default::default()
            }),
            Err(_) => Err(err.into_message()),
        },
        Err(err) => Err(err.into_message()),
    }
}

pub async fn fetch_exit_ip_v4() -> Result<Ipv4Addr, String> {
    let body = request_endpoint(IPV4_URL, false, STACK_PROBE_TIMEOUT, "ip.sb")
        .await
        .map_err(FetchError::into_message)?;
    body.trim()
        .parse::<Ipv4Addr>()
        .map_err(|_| "未返回有效 IPv4 地址".to_owned())
}

pub async fn fetch_exit_ip_v6() -> Result<Ipv6Addr, String> {
    let body = request_endpoint(IPV6_URL, false, STACK_PROBE_TIMEOUT, "ip.sb")
        .await
        .map_err(FetchError::into_message)?;
    body.trim()
        .parse::<Ipv6Addr>()
        .map_err(|_| "未返回有效 IPv6 地址".to_owned())
}

pub async fn lookup_geo(ip: &str) -> Result<GeoIp, String> {
    let addr: IpAddr = ip.trim().parse().map_err(|_| "无效的 IP 地址".to_owned())?;
    if let Ok(mut guard) = cache().lock()
        && let Some(hit) = cache_get(&mut guard, addr, Instant::now())
    {
        return Ok(hit);
    }
    let url = format!("{GEOIP_URL}/{addr}");
    let body = request_endpoint(&url, true, GEO_TIMEOUT, "ip.sb")
        .await
        .map_err(FetchError::into_message)?;
    let geo = parse_geoip_body(&body)?;
    if let Ok(mut guard) = cache().lock() {
        cache_put(&mut guard, addr, geo.clone(), Instant::now());
    }
    Ok(geo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipip_json_text_and_ip_only_responses() {
        let json_payload =
            r#"{"ret":"ok","data":{"ip":"240e:39d::1","location":["中国","北京","北京","","电信"]}}"#;
        let parsed = parse_ipip_json(json_payload).unwrap();
        assert_eq!(parsed.ip, "240e:39d::1");
        assert_eq!(parsed.location, vec!["中国", "北京", "电信"]);
        assert_eq!(parsed.exit_line(), "240e:39d::1 中国 北京 电信");

        let text = "当前 IP：1.2.3.4  来自于：中国 广东 深圳  电信";
        let from_text = parse_ipip_text(text).unwrap();
        assert_eq!(from_text.ip, "1.2.3.4");
        assert_eq!(from_text.location, vec!["中国", "广东", "深圳", "电信"]);

        let ip_only = parse_ipip_ip_json(r#"{"ip":"1.2.3.4"}"#).unwrap();
        assert_eq!(ip_only.ip, "1.2.3.4");
        assert!(ip_only.location.is_empty());

        assert!(parse_ipip_json(r#"{"ret":"err","data":{"ip":"1.2.3.4"}}"#).is_err());
        assert!(parse_ipip_text("当前 IP：not-an-ip 来自于：中国").is_err());
        assert!(parse_ipip_ip_json(r#"{"ip":"bad"}"#).is_err());
    }

    #[test]
    fn parses_full_geoip_payload_from_docs() {
        let payload = r#"{
            "region": "New York",
            "organization": "iCloud Private Relay",
            "region_code": "NY",
            "isp": "iCloud Private Relay",
            "city": "New York",
            "asn_organization": "Akamai Technologies, Inc.",
            "postal_code": "10118",
            "asn": 36183,
            "latitude": 40.7126,
            "ip": "172.225.7.7",
            "continent_code": "NA",
            "offset": -18000,
            "country": "United States",
            "timezone": "America/New_York",
            "country_code": "US",
            "longitude": -74.0066
        }"#;
        let geo = parse_geoip_body(payload).unwrap();
        assert_eq!(geo.ip, "172.225.7.7");
        assert_eq!(geo.asn, Some(36183));
        assert_eq!(geo.offset, Some(-18000));
        assert_eq!(geo.timezone.as_deref(), Some("America/New_York"));
        assert_eq!(
            geo.overview_location(),
            vec!["United States", "New York", "iCloud Private Relay"]
        );
        assert_eq!(
            geo.summary(),
            "United States New York iCloud Private Relay Akamai Technologies, Inc."
        );
        assert_eq!(
            geo.exit_line(),
            "172.225.7.7 United States New York iCloud Private Relay AS36183"
        );
    }

    #[test]
    fn handles_sparse_fields_and_provider_fallback() {
        let payload = r#"{
            "ip": "2001:db8::2",
            "country_code": "JP",
            "isp": "Example ISP"
        }"#;
        let geo = parse_geoip_body(payload).unwrap();
        assert_eq!(geo.ip, "2001:db8::2");
        assert_eq!(geo.provider(), Some("Example ISP"));
        assert_eq!(geo.overview_location(), vec!["JP", "Example ISP"]);
        assert_eq!(geo.summary(), "Example ISP");
    }

    #[test]
    fn deduplicates_identical_location_and_provider_names() {
        let payload = r#"{
            "ip": "126.224.109.234",
            "country": "Singapore",
            "region": "Singapore",
            "city": "Singapore",
            "organization": "SoftBank Corp.",
            "isp": "SoftBank Corp.",
            "asn_organization": "SoftBank Corp."
        }"#;
        let geo = parse_geoip_body(payload).unwrap();
        assert_eq!(geo.overview_location(), vec!["Singapore", "SoftBank Corp."]);
        assert_eq!(geo.summary(), "Singapore SoftBank Corp.");
    }

    #[test]
    fn rejects_invalid_ip_in_payload() {
        assert!(parse_geoip_body(r#"{"ip":"not-an-ip"}"#).is_err());
        assert!(parse_geoip_body(r#"{"country":"US"}"#).is_err());
        assert!(parse_plain_ip("invalid").is_err());
        assert_eq!(
            parse_plain_ip(" 192.0.2.2 \n").unwrap(),
            "192.0.2.2".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn translates_documented_http_errors_and_fallback_policy() {
        assert_eq!(
            describe_http_error(
                StatusCode::BAD_REQUEST,
                r#"{"code":401,"message":"Input string is not a valid IP address"}"#,
                "ip.sb",
            ),
            "无效的 IP 地址"
        );
        assert_eq!(
            describe_http_error(StatusCode::TOO_MANY_REQUESTS, "", "ip.sb"),
            "请求过于频繁，请稍后再试（HTTP 429）"
        );
        assert_eq!(
            describe_http_error(StatusCode::FORBIDDEN, "blocked", "ip.sb"),
            "请求被 ip.sb 拒绝（HTTP 403）"
        );
        assert_eq!(
            describe_http_error(StatusCode::FORBIDDEN, "blocked", "ipip.net"),
            "请求被 ipip.net 拒绝（HTTP 403）"
        );
        assert!(FetchError::Parse("bad".into()).should_fallback());
        assert!(
            FetchError::Http {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "500".into()
            }
            .should_fallback()
        );
        assert!(
            !FetchError::Http {
                status: StatusCode::TOO_MANY_REQUESTS,
                message: "429".into()
            }
            .should_fallback()
        );
        assert!(
            !FetchError::Http {
                status: StatusCode::FORBIDDEN,
                message: "403".into()
            }
            .should_fallback()
        );
        assert!(!FetchError::Network("timeout".into()).should_fallback());
    }

    #[test]
    fn cache_returns_fresh_entries_and_evicts_expired_or_oldest() {
        let mut map = HashMap::new();
        let now = Instant::now();
        let ip1: IpAddr = "1.1.1.1".parse().unwrap();
        let geo1 = GeoIp {
            ip: "1.1.1.1".into(),
            country: Some("Australia".into()),
            ..Default::default()
        };
        cache_put(&mut map, ip1, geo1.clone(), now);
        assert_eq!(cache_get(&mut map, ip1, now + Duration::from_secs(10)), Some(geo1));
        assert_eq!(
            cache_get(&mut map, ip1, now + CACHE_TTL + Duration::from_secs(1)),
            None
        );
        assert!(map.is_empty());

        for i in 0..=CACHE_MAX_ENTRIES {
            let ip = IpAddr::V4(Ipv4Addr::new(198, 51, (i / 256) as u8, (i % 256) as u8));
            let geo = GeoIp {
                ip: ip.to_string(),
                ..Default::default()
            };
            cache_put(&mut map, ip, geo, now + Duration::from_secs(i as u64));
        }
        assert_eq!(map.len(), CACHE_MAX_ENTRIES);
        assert!(!map.contains_key(&IpAddr::V4(Ipv4Addr::new(198, 51, 0, 0))));
    }
}
