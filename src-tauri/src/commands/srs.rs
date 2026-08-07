use flate2::read::ZlibDecoder;
use serde::Serialize;
use std::io::{self, Cursor, Read};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::str::FromStr;

// ---- low-level readers ----

fn read_u8<R: Read>(r: &mut R) -> io::Result<u8> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn read_u64_be<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_be_bytes(buf))
}

fn read_uvarint<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut x = 0u64;
    let mut shift = 0u32;
    loop {
        let b = read_u8(r)?;
        x |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok(x);
        }
        shift += 7;
        if shift >= 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "uvarint overflow",
            ));
        }
    }
}

fn skip_exact<R: Read>(r: &mut R, n: usize) -> io::Result<()> {
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)
}

// ---- string list ----

fn read_string_list<R: Read>(r: &mut R) -> io::Result<Vec<String>> {
    let count = read_uvarint(r)? as usize;
    let mut v = Vec::with_capacity(count);
    for _ in 0..count {
        let len = read_uvarint(r)? as usize;
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf)?;
        v.push(String::from_utf8_lossy(&buf).into_owned());
    }
    Ok(v)
}

fn skip_string_list<R: Read>(r: &mut R) -> io::Result<()> {
    let count = read_uvarint(r)? as usize;
    for _ in 0..count {
        let len = read_uvarint(r)? as usize;
        skip_exact(r, len)?;
    }
    Ok(())
}

fn skip_u16_list<R: Read>(r: &mut R) -> io::Result<()> {
    let count = read_uvarint(r)? as usize;
    skip_exact(r, count * 2)
}

fn skip_u8_list<R: Read>(r: &mut R) -> io::Result<()> {
    let count = read_uvarint(r)? as usize;
    skip_exact(r, count)
}

// ---- succinct trie (LOUDS) ----
// Ported from github.com/sagernet/sing/common/domain/set.go and matcher.go

fn get_bit(bm: &[u64], i: usize) -> bool {
    let w = i >> 6;
    w < bm.len() && bm[w] & (1u64 << (i & 63)) != 0
}

fn count_zeros(bm: &[u64], i: usize) -> usize {
    let full_words = i >> 6;
    let rem = i & 63;
    let mut ones = 0usize;
    for wi in 0..full_words.min(bm.len()) {
        ones += bm[wi].count_ones() as usize;
    }
    if rem > 0 && full_words < bm.len() {
        ones += (bm[full_words] & ((1u64 << rem) - 1)).count_ones() as usize;
    }
    i - ones
}

fn select_ith_one(bm: &[u64], target: usize) -> usize {
    let mut remaining = target;
    for (wi, &word) in bm.iter().enumerate() {
        let ones = word.count_ones() as usize;
        if ones > remaining {
            let mut w = word;
            for _ in 0..remaining {
                w &= w - 1; // clear lowest set bit
            }
            return wi * 64 + w.trailing_zeros() as usize;
        }
        remaining -= ones;
    }
    bm.len() * 64
}

fn read_u64_slice<R: Read>(r: &mut R) -> io::Result<Vec<u64>> {
    let count = read_uvarint(r)? as usize;
    let mut v = vec![0u64; count];
    for x in &mut v {
        *x = read_u64_be(r)?;
    }
    Ok(v)
}

fn read_byte_slice<R: Read>(r: &mut R) -> io::Result<Vec<u8>> {
    let count = read_uvarint(r)? as usize;
    let mut buf = vec![0u8; count];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

struct SuccinctSet {
    leaves: Vec<u64>,
    label_bitmap: Vec<u64>,
    labels: Vec<u8>,
}

impl SuccinctSet {
    fn read<R: Read>(r: &mut R) -> io::Result<Self> {
        let _ver = read_u8(r)?; // always 0
        let leaves = read_u64_slice(r)?;
        let label_bitmap = read_u64_slice(r)?;
        let labels = read_byte_slice(r)?;
        Ok(SuccinctSet {
            leaves,
            label_bitmap,
            labels,
        })
    }

    /// Match a domain against the trie.
    /// Domains are stored reversed. Special labels:
    ///   '\r' (0x0D) = PREFIX_LABEL: any suffix matches
    ///   '\n' (0x0A) = ROOT_LABEL:   root domain suffix matches
    fn match_domain(&self, domain: &str) -> bool {
        const PREFIX: u8 = b'\r';
        const ROOT: u8 = b'\n';

        let key: Vec<u8> = domain.bytes().rev().collect();
        let mut node_id = 0usize;
        let mut bm_idx = 0usize;

        for &ch in &key {
            loop {
                if get_bit(&self.label_bitmap, bm_idx) {
                    return false;
                }
                let li = bm_idx - node_id;
                if li >= self.labels.len() {
                    return false;
                }
                let lbl = self.labels[li];
                if lbl == PREFIX {
                    return true;
                }
                if lbl == ROOT {
                    let child = count_zeros(&self.label_bitmap, bm_idx + 1);
                    if ch == b'.' && get_bit(&self.leaves, child) {
                        return true;
                    }
                }
                if lbl == ch {
                    break;
                }
                bm_idx += 1;
            }
            node_id = count_zeros(&self.label_bitmap, bm_idx + 1);
            // node_id >= 1 here since bm[bm_idx] was 0
            bm_idx = select_ith_one(&self.label_bitmap, node_id - 1) + 1;
        }

        if get_bit(&self.leaves, node_id) {
            return true;
        }
        loop {
            if get_bit(&self.label_bitmap, bm_idx) {
                return false;
            }
            let li = bm_idx - node_id;
            if li >= self.labels.len() {
                return false;
            }
            let lbl = self.labels[li];
            if lbl == PREFIX || lbl == ROOT {
                return true;
            }
            bm_idx += 1;
        }
    }
}

struct AdGuardMatcher {
    set: SuccinctSet,
}

impl AdGuardMatcher {
    const PREFIX: u8 = b'\r';
    const ROOT: u8 = b'\n';
    const ANY: u8 = b'*';
    const SUFFIX: u8 = b'\x08';
    const MAX_DEPTH: usize = 100;

    fn read<R: Read>(r: &mut R) -> io::Result<Self> {
        Ok(Self {
            set: SuccinctSet::read(r)?,
        })
    }

    fn match_domain(&self, domain: &str) -> bool {
        let mut key = domain.bytes().rev().collect::<Vec<u8>>();
        if self.has(&key, 0, 0, 0) {
            return true;
        }
        loop {
            let mut with_suffix = Vec::with_capacity(1 + key.len());
            with_suffix.push(Self::SUFFIX);
            with_suffix.extend_from_slice(&key);
            if self.has(&with_suffix, 0, 0, 0) {
                return true;
            }
            if let Some(idx) = key.iter().position(|&b| b == b'.') {
                key = key[idx + 1..].to_vec();
            } else {
                return false;
            }
        }
    }

    fn has(&self, key: &[u8], mut node_id: usize, mut bm_idx: usize, depth: usize) -> bool {
        if depth > Self::MAX_DEPTH {
            return false;
        }

        for i in 0..key.len() {
            let ch = key[i];
            loop {
                if get_bit(&self.set.label_bitmap, bm_idx) {
                    return false;
                }
                let li = bm_idx.saturating_sub(node_id);
                if li >= self.set.labels.len() {
                    return false;
                }
                let lbl = self.set.labels[li];
                if lbl == Self::PREFIX {
                    return true;
                }
                if lbl == Self::ROOT {
                    let child = count_zeros(&self.set.label_bitmap, bm_idx + 1);
                    if ch == b'.' && get_bit(&self.set.leaves, child) {
                        return true;
                    }
                }
                if lbl == ch {
                    break;
                }
                if lbl == Self::ANY || lbl == Self::SUFFIX {
                    let next_node = count_zeros(&self.set.label_bitmap, bm_idx + 1);
                    let next_bm =
                        select_ith_one(&self.set.label_bitmap, next_node.saturating_sub(1)) + 1;
                    if self.has(&key[i..], next_node, next_bm, depth + 1) {
                        return true;
                    }
                    for j in (i + 1)..=key.len() {
                        if self.has(&key[j..], next_node, next_bm, depth + 1) {
                            return true;
                        }
                    }
                }
                bm_idx += 1;
            }
            node_id = count_zeros(&self.set.label_bitmap, bm_idx + 1);
            bm_idx = select_ith_one(&self.set.label_bitmap, node_id.saturating_sub(1)) + 1;
        }

        if get_bit(&self.set.leaves, node_id) {
            return true;
        }
        loop {
            if get_bit(&self.set.label_bitmap, bm_idx) {
                return false;
            }
            let li = bm_idx.saturating_sub(node_id);
            if li >= self.set.labels.len() {
                return false;
            }
            let lbl = self.set.labels[li];
            if lbl == Self::PREFIX || lbl == Self::ROOT || lbl == Self::SUFFIX {
                return true;
            }
            if lbl == Self::ANY {
                let next_node = count_zeros(&self.set.label_bitmap, bm_idx + 1);
                let next_bm =
                    select_ith_one(&self.set.label_bitmap, next_node.saturating_sub(1)) + 1;
                return self.has(&[], next_node, next_bm, depth + 1);
            }
            bm_idx += 1;
        }
    }
}

// ---- IP set ----
// Format (ip_set.go): version(u8=1) + count(u64 BE) + [uvarint+bytes, uvarint+bytes]*

struct IpSet {
    ranges_v4: Vec<(u32, u32)>,
    ranges_v6: Vec<(u128, u128)>,
}

fn read_ip_set<R: Read>(r: &mut R) -> io::Result<IpSet> {
    let _ver = read_u8(r)?;
    let count = read_u64_be(r)? as usize;
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for _ in 0..count {
        let fl = read_uvarint(r)? as usize;
        let mut from = vec![0u8; fl];
        r.read_exact(&mut from)?;
        let tl = read_uvarint(r)? as usize;
        let mut to = vec![0u8; tl];
        r.read_exact(&mut to)?;
        match (fl, tl) {
            (4, 4) => v4.push((
                u32::from_be_bytes([from[0], from[1], from[2], from[3]]),
                u32::from_be_bytes([to[0], to[1], to[2], to[3]]),
            )),
            (16, 16) => {
                let mut fa = [0u8; 16];
                let mut ta = [0u8; 16];
                fa.copy_from_slice(&from);
                ta.copy_from_slice(&to);
                v6.push((u128::from_be_bytes(fa), u128::from_be_bytes(ta)));
            }
            _ => {}
        }
    }
    Ok(IpSet {
        ranges_v4: v4,
        ranges_v6: v6,
    })
}

fn skip_ip_set<R: Read>(r: &mut R) -> io::Result<()> {
    read_u8(r)?;
    let count = read_u64_be(r)? as usize;
    for _ in 0..count {
        let fl = read_uvarint(r)? as usize;
        skip_exact(r, fl)?;
        let tl = read_uvarint(r)? as usize;
        skip_exact(r, tl)?;
    }
    Ok(())
}

impl IpSet {
    fn contains(&self, ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) => {
                let n = u32::from_be_bytes(v4.octets());
                self.ranges_v4.iter().any(|&(f, t)| n >= f && n <= t)
            }
            IpAddr::V6(v6) => {
                let n = u128::from_be_bytes(v6.octets());
                self.ranges_v6.iter().any(|&(f, t)| n >= f && n <= t)
            }
        }
    }

    /// True when `[from, to]` shares at least one address with the set.
    fn overlaps(&self, from: &IpAddr, to: &IpAddr) -> bool {
        match (from, to) {
            (IpAddr::V4(a), IpAddr::V4(b)) => {
                let lo = u32::from_be_bytes(a.octets());
                let hi = u32::from_be_bytes(b.octets());
                self.ranges_v4.iter().any(|&(f, t)| lo <= t && hi >= f)
            }
            (IpAddr::V6(a), IpAddr::V6(b)) => {
                let lo = u128::from_be_bytes(a.octets());
                let hi = u128::from_be_bytes(b.octets());
                self.ranges_v6.iter().any(|&(f, t)| lo <= t && hi >= f)
            }
            _ => false,
        }
    }

    fn matches(&self, query: &Query) -> bool {
        match query {
            Query::Ip(ip) => self.contains(ip),
            Query::IpRange(from, to) => self.overlaps(from, to),
            Query::Domain(_) => false,
        }
    }
}

// ---- rule item type constants ----

const ITEM_QUERY_TYPE: u8 = 0x00;
const ITEM_NETWORK: u8 = 0x01;
const ITEM_DOMAIN: u8 = 0x02;
const ITEM_DOMAIN_KEYWORD: u8 = 0x03;
const ITEM_DOMAIN_REGEX: u8 = 0x04;
const ITEM_SOURCE_IP_CIDR: u8 = 0x05;
const ITEM_IP_CIDR: u8 = 0x06;
const ITEM_SOURCE_PORT: u8 = 0x07;
const ITEM_SOURCE_PORT_RANGE: u8 = 0x08;
const ITEM_PORT: u8 = 0x09;
const ITEM_PORT_RANGE: u8 = 0x0A;
const ITEM_PROCESS_NAME: u8 = 0x0B;
const ITEM_PROCESS_PATH: u8 = 0x0C;
const ITEM_PACKAGE_NAME: u8 = 0x0D;
const ITEM_WIFI_SSID: u8 = 0x0E;
const ITEM_WIFI_BSSID: u8 = 0x0F;
const ITEM_ADGUARD_DOMAIN: u8 = 0x10;
const ITEM_PROCESS_PATH_REGEX: u8 = 0x11;
const ITEM_NETWORK_TYPE: u8 = 0x12;
const ITEM_NETWORK_IS_EXPENSIVE: u8 = 0x13;
const ITEM_NETWORK_IS_CONSTRAINED: u8 = 0x14;
const ITEM_NETWORK_INTERFACE_ADDRESS: u8 = 0x15;
const ITEM_DEFAULT_INTERFACE_ADDRESS: u8 = 0x16;
const ITEM_FINAL: u8 = 0xFF;

// ---- query ----

enum Query {
    Domain(String),
    Ip(IpAddr),
    /// Inclusive address range, both bounds of the same family.
    IpRange(IpAddr, IpAddr),
}

fn prefix_range_v4(addr: Ipv4Addr, prefix: u32) -> (IpAddr, IpAddr) {
    let n = u32::from_be_bytes(addr.octets());
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    let network = n & mask;
    (
        IpAddr::V4(Ipv4Addr::from(network)),
        IpAddr::V4(Ipv4Addr::from(network | !mask)),
    )
}

fn prefix_range_v6(addr: Ipv6Addr, prefix: u32) -> (IpAddr, IpAddr) {
    let n = u128::from_be_bytes(addr.octets());
    let mask = if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    };
    let network = n & mask;
    (
        IpAddr::V6(Ipv6Addr::from(network)),
        IpAddr::V6(Ipv6Addr::from(network | !mask)),
    )
}

/// Parse `1.0.1.240/29` or `240e:e1:a800::/36` into its address range.
fn parse_cidr(s: &str) -> Option<(IpAddr, IpAddr)> {
    let (addr_str, prefix_str) = s.split_once('/')?;
    let prefix: u32 = prefix_str.trim().parse().ok()?;
    match IpAddr::from_str(addr_str.trim()).ok()? {
        IpAddr::V4(v4) if prefix <= 32 => Some(prefix_range_v4(v4, prefix)),
        IpAddr::V6(v6) if prefix <= 128 => Some(prefix_range_v6(v6, prefix)),
        _ => None,
    }
}

/// Parse a truncated address as the prefix it implies:
/// `1.0.1` -> `1.0.1.0/24`, `240e:e1:a800` -> `240e:e1:a800::/48`.
fn parse_partial_ip(s: &str) -> Option<(IpAddr, IpAddr)> {
    if s.contains(':') {
        let body = s.strip_suffix(':').unwrap_or(s);
        if body.contains("::") {
            return None;
        }
        let groups: Vec<&str> = body.split(':').collect();
        if groups.len() < 2 || groups.len() >= 8 {
            return None;
        }
        let mut segments = [0u16; 8];
        for (i, g) in groups.iter().enumerate() {
            if g.is_empty() || g.len() > 4 || !g.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            segments[i] = u16::from_str_radix(g, 16).ok()?;
        }
        let addr = Ipv6Addr::from(segments);
        Some(prefix_range_v6(addr, groups.len() as u32 * 16))
    } else {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() < 2 || parts.len() >= 4 {
            return None;
        }
        let mut octets = [0u8; 4];
        for (i, p) in parts.iter().enumerate() {
            if p.is_empty() || p.len() > 3 || !p.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            octets[i] = p.parse().ok()?;
        }
        let addr = Ipv4Addr::from(octets);
        Some(prefix_range_v4(addr, parts.len() as u32 * 8))
    }
}

fn parse_query(q: &str) -> Query {
    let q = q.trim();
    if let Ok(ip) = IpAddr::from_str(q) {
        return Query::Ip(ip);
    }
    if let Some((from, to)) = parse_cidr(q).or_else(|| parse_partial_ip(q)) {
        return Query::IpRange(from, to);
    }
    Query::Domain(q.to_lowercase())
}

// ---- rule matching ----

/// Match a DefaultHeadlessRule.
/// Conditions of the same type are ANDed. Conditions of a non-matching type are ignored.
fn match_default_rule<R: Read>(r: &mut R, query: &Query) -> io::Result<bool> {
    let mut domain_seen = false;
    let mut domain_matched = false;
    let mut ip_seen = false;
    let mut ip_matched = false;
    loop {
        let item_type = read_u8(r)?;
        match item_type {
            ITEM_FINAL => {
                let invert = read_u8(r)? != 0;
                let result = match query {
                    Query::Domain(_) => domain_seen && domain_matched,
                    Query::Ip(_) | Query::IpRange(..) => ip_seen && ip_matched,
                };
                return Ok(if invert { !result } else { result });
            }
            ITEM_DOMAIN => {
                let set = SuccinctSet::read(r)?;
                let m = if let Query::Domain(d) = query {
                    set.match_domain(d)
                } else {
                    false
                };
                domain_seen = true;
                domain_matched = domain_matched || m;
            }
            ITEM_DOMAIN_KEYWORD => {
                let kws = read_string_list(r)?;
                let m = if let Query::Domain(d) = query {
                    kws.iter().any(|kw| d.contains(kw.as_str()))
                } else {
                    false
                };
                domain_seen = true;
                domain_matched = domain_matched || m;
            }
            ITEM_DOMAIN_REGEX => {
                skip_string_list(r)?;
                if matches!(query, Query::Domain(_)) {
                    domain_seen = true;
                }
            }
            ITEM_IP_CIDR => {
                let set = read_ip_set(r)?;
                let m = set.matches(query);
                ip_seen = true;
                ip_matched = ip_matched || m;
            }
            ITEM_SOURCE_IP_CIDR => {
                skip_ip_set(r)?;
            }
            ITEM_QUERY_TYPE => {
                skip_u16_list(r)?;
            }
            ITEM_NETWORK => {
                skip_string_list(r)?;
            }
            ITEM_SOURCE_PORT | ITEM_PORT => {
                skip_u16_list(r)?;
            }
            ITEM_SOURCE_PORT_RANGE
            | ITEM_PORT_RANGE
            | ITEM_PROCESS_NAME
            | ITEM_PROCESS_PATH
            | ITEM_PACKAGE_NAME
            | ITEM_WIFI_SSID
            | ITEM_WIFI_BSSID
            | ITEM_PROCESS_PATH_REGEX => {
                skip_string_list(r)?;
            }
            ITEM_ADGUARD_DOMAIN => {
                let matcher = AdGuardMatcher::read(r)?;
                let m = if let Query::Domain(d) = query {
                    matcher.match_domain(d)
                } else {
                    false
                };
                domain_seen = true;
                domain_matched = domain_matched || m;
            }
            ITEM_NETWORK_TYPE => {
                skip_u8_list(r)?;
            }
            ITEM_NETWORK_IS_EXPENSIVE | ITEM_NETWORK_IS_CONSTRAINED => {
                // no data
            }
            ITEM_NETWORK_INTERFACE_ADDRESS => {
                let size = read_uvarint(r)? as usize;
                for _ in 0..size {
                    read_u8(r)?; // key
                    let count = read_uvarint(r)? as usize;
                    for _ in 0..count {
                        let addr_len = read_uvarint(r)? as usize;
                        skip_exact(r, addr_len)?;
                        skip_exact(r, 1)?; // prefix bits
                    }
                }
            }
            ITEM_DEFAULT_INTERFACE_ADDRESS => {
                let count = read_uvarint(r)? as usize;
                for _ in 0..count {
                    let addr_len = read_uvarint(r)? as usize;
                    skip_exact(r, addr_len)?;
                    skip_exact(r, 1)?;
                }
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown item type: {:#x}", other),
                ));
            }
        }
    }
}

fn match_rule<R: Read>(r: &mut R, query: &Query) -> io::Result<bool> {
    match read_u8(r)? {
        0 => match_default_rule(r, query),
        1 => match_logical_rule(r, query),
        t => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown rule type: {}", t),
        )),
    }
}

fn match_logical_rule<R: Read>(r: &mut R, query: &Query) -> io::Result<bool> {
    let mode = read_u8(r)?; // 0 = AND, 1 = OR
    let count = read_uvarint(r)? as usize;
    let mut results = Vec::with_capacity(count);
    for _ in 0..count {
        results.push(match_rule(r, query)?);
    }
    let invert = read_u8(r)? != 0;
    let result = if mode == 0 {
        results.iter().all(|&x| x)
    } else {
        results.iter().any(|&x| x)
    };
    Ok(if invert { !result } else { result })
}

// ---- SRS matching core ----

fn srs_match_bytes(data: &[u8], query: &Query) -> Result<bool, String> {
    if data.len() < 4 || &data[0..3] != b"SRS" {
        return Err("invalid SRS magic".into());
    }
    let mut decompressed = Vec::new();
    ZlibDecoder::new(&data[4..])
        .read_to_end(&mut decompressed)
        .map_err(|e| format!("decompress: {}", e))?;
    let mut cur = Cursor::new(&decompressed);
    let rule_count =
        read_uvarint(&mut cur).map_err(|e| format!("read rule count: {}", e))? as usize;
    for i in 0..rule_count {
        match match_rule(&mut cur, query) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(e) => return Err(format!("rule[{}]: {}", i, e)),
        }
    }
    Ok(false)
}

// ---- BoltDB minimal reader ----
// Reads bbolt database files to extract rule set content from the rule_set bucket.

const PAGE_HEADER_SIZE: usize = 16; // pgid(8) + flags(2) + count(2) + overflow(4)
const BRANCH_ELEM_SIZE: usize = 16; // pos(4) + ksize(4) + pgid(8)
const LEAF_ELEM_SIZE: usize = 16; // flags(4) + pos(4) + ksize(4) + vsize(4)
const BRANCH_PAGE_FLAG: u16 = 0x01;
const LEAF_PAGE_FLAG: u16 = 0x02;
const BUCKET_LEAF_FLAG: u32 = 0x01;

/// Read the page size from the meta page (byte offset 24 in the file).
fn bolt_page_size(data: &[u8]) -> usize {
    if data.len() < 28 {
        return 4096;
    }
    let ps = u32::from_le_bytes(data[24..28].try_into().unwrap()) as usize;
    if ps >= 512 && ps.is_power_of_two() {
        ps
    } else {
        4096
    }
}

/// Read the root bucket's pgid from the meta page with the highest txid.
/// Meta page layout (offsets within the page):
///   16: magic(u32), 20: version(u32), 24: pageSize(u32), 28: flags(u32)
///   32: root.root(u64), 40: root.sequence(u64)
///   48: freelist(u64), 56: pgid(u64), 64: txid(u64), 72: checksum(u64)
fn bolt_meta_root(data: &[u8], ps: usize) -> u64 {
    let read_u64_le = |off: usize| -> u64 {
        if off + 8 > data.len() {
            return 0;
        }
        u64::from_le_bytes(data[off..off + 8].try_into().unwrap())
    };
    let root0 = read_u64_le(32);
    let txid0 = read_u64_le(64);
    let root1 = read_u64_le(ps + 32);
    let txid1 = read_u64_le(ps + 64);
    if txid1 >= txid0 { root1 } else { root0 }
}

/// Navigate the B-tree rooted at `pgid` to find `key`.
/// Returns `Some((is_bucket, value_bytes))` if found, `None` if not found.
fn bolt_btree_lookup(
    data: &[u8],
    ps: usize,
    pgid: u64,
    key: &[u8],
) -> io::Result<Option<(bool, Vec<u8>)>> {
    let mut curr = pgid;
    loop {
        let off = curr as usize * ps;
        if off + PAGE_HEADER_SIZE > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("page {} out of bounds (file size {})", curr, data.len()),
            ));
        }
        let overflow = u32::from_le_bytes(data[off + 12..off + 16].try_into().unwrap()) as usize;
        let page_end = off + ps * (1 + overflow);
        if page_end > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "overflow page extends beyond file",
            ));
        }
        let page = &data[off..page_end];
        let flags = u16::from_le_bytes([page[8], page[9]]);
        let count = u16::from_le_bytes([page[10], page[11]]) as usize;

        if flags & BRANCH_PAGE_FLAG != 0 {
            curr = bolt_branch_child(page, count, key);
        } else if flags & LEAF_PAGE_FLAG != 0 {
            return Ok(bolt_leaf_lookup(page, count, key));
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected page flags {:#x} at pgid {}", flags, curr),
            ));
        }
    }
}

/// Find the child pgid for `key` in a branch page.
/// Implements the same binary search as bbolt cursor.searchBranch.
fn bolt_branch_child(page: &[u8], count: usize, key: &[u8]) -> u64 {
    if count == 0 {
        return 0;
    }
    // Binary search: find first element index where elem.key >= key
    let mut lo = 0usize;
    let mut hi = count;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let elem_start = PAGE_HEADER_SIZE + mid * BRANCH_ELEM_SIZE;
        if elem_start + BRANCH_ELEM_SIZE > page.len() {
            hi = mid;
            continue;
        }
        let pos = u32::from_le_bytes(page[elem_start..elem_start + 4].try_into().unwrap()) as usize;
        let ksize =
            u32::from_le_bytes(page[elem_start + 4..elem_start + 8].try_into().unwrap()) as usize;
        let k_end = elem_start + pos + ksize;
        if k_end > page.len() {
            hi = mid;
            continue;
        }
        let k = &page[elem_start + pos..k_end];
        if k < key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    // lo = first index where elem.key >= key
    // if exact match: use lo; if not exact and lo > 0: use lo-1; else use 0
    let is_exact = lo < count && {
        let es = PAGE_HEADER_SIZE + lo * BRANCH_ELEM_SIZE;
        let pos = u32::from_le_bytes(page[es..es + 4].try_into().unwrap()) as usize;
        let ksize = u32::from_le_bytes(page[es + 4..es + 8].try_into().unwrap()) as usize;
        es + pos + ksize <= page.len() && &page[es + pos..es + pos + ksize] == key
    };
    let idx = if is_exact {
        lo
    } else if lo > 0 {
        lo - 1
    } else {
        0
    };
    let elem_start = PAGE_HEADER_SIZE + idx * BRANCH_ELEM_SIZE;
    u64::from_le_bytes(page[elem_start + 8..elem_start + 16].try_into().unwrap())
}

/// Binary search for `key` in a leaf page.
/// Returns `Some((is_bucket, value_bytes))` if found.
fn bolt_leaf_lookup(page: &[u8], count: usize, key: &[u8]) -> Option<(bool, Vec<u8>)> {
    let mut lo = 0usize;
    let mut hi = count;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let elem_start = PAGE_HEADER_SIZE + mid * LEAF_ELEM_SIZE;
        if elem_start + LEAF_ELEM_SIZE > page.len() {
            break;
        }
        let elem_flags = u32::from_le_bytes(page[elem_start..elem_start + 4].try_into().unwrap());
        let pos =
            u32::from_le_bytes(page[elem_start + 4..elem_start + 8].try_into().unwrap()) as usize;
        let ksize =
            u32::from_le_bytes(page[elem_start + 8..elem_start + 12].try_into().unwrap()) as usize;
        let vsize =
            u32::from_le_bytes(page[elem_start + 12..elem_start + 16].try_into().unwrap()) as usize;
        let k_start = elem_start + pos;
        let k_end = k_start + ksize;
        if k_end > page.len() {
            break;
        }
        let k = &page[k_start..k_end];
        match k.cmp(key) {
            std::cmp::Ordering::Equal => {
                let v_start = k_end;
                let v_end = v_start + vsize;
                let v = if v_end <= page.len() {
                    page[v_start..v_end].to_vec()
                } else {
                    vec![]
                };
                return Some((elem_flags & BUCKET_LEAF_FLAG != 0, v));
            }
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

// ---- SavedBinary parser ----
// Format: u8(1) + uvarint(hash_len) + hash + uvarint(content_len) + content + i64(timestamp) + uvarint(etag_len) + etag

fn parse_saved_binary_content(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut cur = Cursor::new(data);
    let ver = read_u8(&mut cur)?;
    if ver != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected SavedBinary version: {}", ver),
        ));
    }
    // skip hash (md5 = 16 bytes, stored as uvarint(16) + 16 bytes)
    let hash_len = read_uvarint(&mut cur)? as usize;
    skip_exact(&mut cur, hash_len)?;
    // read content
    let content_len = read_uvarint(&mut cur)? as usize;
    let mut content = vec![0u8; content_len];
    cur.read_exact(&mut content)?;
    Ok(content)
}

// ---- cache.db auto-discovery ----

fn collect_candidate_dirs(candidates: &[&str]) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();

    for s in candidates {
        if s.is_empty() {
            continue;
        }
        let p = std::path::Path::new(s);
        // If candidate is a file path, use its parent directory
        let dir = if p.is_file() {
            if let Some(parent) = p.parent() {
                parent.to_path_buf()
            } else {
                continue;
            }
        } else {
            p.to_path_buf()
        };
        if dir.is_dir() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }

    dirs
}

/// Search candidate directories for a `cache.db` file.
/// Checks each directory directly, then one level of subdirectories.
fn find_cache_db(candidates: &[&str]) -> Option<std::path::PathBuf> {
    let dirs = collect_candidate_dirs(candidates);

    for dir in &dirs {
        let candidate = dir.join("cache.db");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // Search one level of subdirectories
    for dir in &dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let sub = entry.path();
                if sub.is_dir() {
                    let candidate = sub.join("cache.db");
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    None
}

/// Find `<tag>.srs` under candidate directories recursively.
/// Includes both direct children and deeper nested subdirectories.
fn find_local_srs_by_tag(candidates: &[&str], tag: &str) -> Option<std::path::PathBuf> {
    fn tag_stem_candidates(tag: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push_unique = |s: String| {
            let n = s.trim().to_ascii_lowercase();
            if !n.is_empty() && !out.contains(&n) {
                out.push(n);
            }
        };

        push_unique(tag.to_string());

        // Common provider suffixes in sing-box ecosystems.
        for suffix in ["-domain", "-ip", "-filter", "-cidr", "-geoip", "-geosite"] {
            if let Some(stripped) = tag.strip_suffix(suffix) {
                push_unique(stripped.to_string());
            }
        }

        // Fallback: drop the last segment after '-'.
        if let Some((head, _)) = tag.rsplit_once('-') {
            push_unique(head.to_string());
        }

        out
    }

    let dirs = collect_candidate_dirs(candidates);
    let mut stack: Vec<(std::path::PathBuf, usize)> = dirs.into_iter().map(|d| (d, 0)).collect();
    let max_depth = 8usize;
    let stem_candidates = tag_stem_candidates(tag);

    while let Some((dir, depth)) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let is_srs = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("srs"))
                    .unwrap_or(false);
                if !is_srs {
                    continue;
                }
                let stem_matches = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| {
                        let stem = s.to_ascii_lowercase();
                        stem_candidates.iter().any(|c| c == &stem)
                    })
                    .unwrap_or(false);
                if stem_matches {
                    return Some(path);
                }
            } else if path.is_dir() && depth < max_depth {
                stack.push((path, depth + 1));
            }
        }
    }
    None
}

fn resolve_path_candidates(base_dirs: &[PathBuf], raw_path: &str) -> Vec<PathBuf> {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        return vec![path.to_path_buf()];
    }

    let mut out = Vec::new();
    for base in base_dirs {
        out.push(base.join(path));
    }
    out
}

/// Resolve local binary rule-set path from config.json by tag.
/// Reads route.rule_set and returns a matching file path when type=local and path exists.
fn find_local_srs_from_config(working_dir: &str, config_path: &str, tag: &str) -> Option<PathBuf> {
    if config_path.is_empty() {
        return None;
    }

    let cfg_path = Path::new(config_path);
    if !cfg_path.is_file() {
        return None;
    }

    let cfg_text = std::fs::read_to_string(cfg_path).ok()?;
    let cfg_json: serde_json::Value = serde_json::from_str(&cfg_text).ok()?;
    let rule_sets = cfg_json
        .get("route")
        .and_then(|r| r.get("rule_set"))
        .and_then(|v| v.as_array())?;

    let mut base_dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = cfg_path.parent() {
        base_dirs.push(parent.to_path_buf());
    }
    if !working_dir.is_empty() {
        base_dirs.push(PathBuf::from(working_dir));
    }
    base_dirs.dedup();

    for item in rule_sets {
        let item_tag = item.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        if item_tag != tag {
            continue;
        }
        let item_type = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("inline");
        if item_type != "local" {
            continue;
        }
        let raw_path = item.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if raw_path.is_empty() {
            continue;
        }
        let fmt = item.get("format").and_then(|v| v.as_str()).unwrap_or("");
        if !fmt.is_empty() && fmt != "binary" {
            continue;
        }

        for candidate in resolve_path_candidates(&base_dirs, raw_path) {
            if candidate.is_file() {
                return Some(candidate);
            }
            if candidate.extension().is_none() {
                let with_srs = candidate.with_extension("srs");
                if with_srs.is_file() {
                    return Some(with_srs);
                }
            }
        }
    }

    None
}

// ---- public Tauri commands ----

#[tauri::command]
pub fn srs_match(path: String, query: String) -> Result<bool, String> {
    let data = std::fs::read(&path).map_err(|e| format!("read '{}': {}", path, e))?;
    let q = parse_query(&query);
    srs_match_bytes(&data, &q)
}

/// Find and return the path to cache.db, searching common locations.
#[tauri::command]
pub fn find_cache_db_path(
    working_dir: String,
    config_path: String,
    singbox_path: String,
) -> Result<String, String> {
    find_cache_db(&[&working_dir, &config_path, &singbox_path])
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| {
            format!(
                "cache.db not found (searched: '{}', '{}', '{}')",
                working_dir, config_path, singbox_path
            )
        })
}

#[tauri::command]
pub fn srs_match_cache(cache_path: String, tag: String, query: String) -> Result<bool, String> {
    let data = std::fs::read(&cache_path).map_err(|e| format!("read '{}': {}", cache_path, e))?;

    let ps = bolt_page_size(&data);
    let root_pgid = bolt_meta_root(&data, ps);

    // Step 1: find "rule_set" bucket in root B-tree
    let (is_bucket, bval) = bolt_btree_lookup(&data, ps, root_pgid, b"rule_set")
        .map_err(|e| format!("lookup rule_set bucket: {}", e))?
        .ok_or_else(|| "rule_set bucket not found in cache.db".to_string())?;

    if !is_bucket || bval.len() < 8 {
        return Err("rule_set is not a valid bucket entry".into());
    }
    let bucket_root = u64::from_le_bytes(bval[0..8].try_into().unwrap());

    // Step 2: find tag entry in rule_set bucket
    let saved_binary_bytes: Vec<u8> = if bucket_root == 0 {
        // Inline bucket: bval[16..] is the inline leaf page
        if bval.len() < 16 + PAGE_HEADER_SIZE {
            return Err(format!("tag '{}' not found (inline bucket too small)", tag));
        }
        let inline_page = &bval[16..];
        let inline_count = u16::from_le_bytes([inline_page[10], inline_page[11]]) as usize;
        bolt_leaf_lookup(inline_page, inline_count, tag.as_bytes())
            .map(|(_, v)| v)
            .ok_or_else(|| format!("tag '{}' not found in cache.db", tag))?
    } else {
        bolt_btree_lookup(&data, ps, bucket_root, tag.as_bytes())
            .map_err(|e| format!("lookup tag '{}': {}", tag, e))?
            .map(|(_, v)| v)
            .ok_or_else(|| format!("tag '{}' not found in cache.db", tag))?
    };

    // Step 3: parse SavedBinary to extract .srs content
    let srs_content = parse_saved_binary_content(&saved_binary_bytes)
        .map_err(|e| format!("parse SavedBinary for '{}': {}", tag, e))?;

    // Step 4: match query against the .srs content
    let q = parse_query(&query);
    srs_match_bytes(&srs_content, &q)
}

// ---- rule enumeration ----

#[derive(Serialize, Clone)]
pub struct RuleEntry {
    #[serde(rename = "type")]
    pub rule_type: String,
    pub value: String,
}

impl SuccinctSet {
    /// DFS traverse the LOUDS trie and collect all stored domains.
    fn enumerate(&self) -> Vec<(String, String)> {
        const PREFIX: u8 = b'\r';
        const ROOT: u8 = b'\n';

        let mut results = Vec::new();
        // Stack: (node_id, bm_idx, path_so_far_reversed, from_root)
        let mut stack: Vec<(usize, usize, Vec<u8>, bool)> = vec![(0, 0, Vec::new(), false)];

        while let Some((node_id, start_bm_idx, path, from_root)) = stack.pop() {
            // First scan children to check for ROOT/PREFIX labels
            let mut has_suffix_marker = false;
            let mut bm_idx = start_bm_idx;
            loop {
                if get_bit(&self.label_bitmap, bm_idx) {
                    break;
                }
                let li = bm_idx.saturating_sub(node_id);
                if li >= self.labels.len() {
                    break;
                }
                let lbl = self.labels[li];
                if lbl == PREFIX || lbl == ROOT {
                    has_suffix_marker = true;
                    break;
                }
                bm_idx += 1;
            }

            // Emit entry for this node
            if !path.is_empty() {
                if has_suffix_marker {
                    // Has ROOT/PREFIX child → domain_suffix
                    let suffix: String = path.iter().rev().map(|&b| b as char).collect();
                    results.push(("domain_suffix".to_string(), suffix));
                } else if !from_root && get_bit(&self.leaves, node_id) {
                    // Pure leaf without suffix marker → domain
                    let domain: String = path.iter().rev().map(|&b| b as char).collect();
                    results.push(("domain".to_string(), domain));
                }
            }

            // Iterate children and recurse
            bm_idx = start_bm_idx;
            loop {
                if get_bit(&self.label_bitmap, bm_idx) {
                    break;
                }
                let li = bm_idx.saturating_sub(node_id);
                if li >= self.labels.len() {
                    break;
                }
                let lbl = self.labels[li];

                if lbl == PREFIX {
                    // Already handled above
                } else if lbl == ROOT {
                    // Recurse into ROOT child for deeper entries
                    let child_node = count_zeros(&self.label_bitmap, bm_idx + 1);
                    if child_node > 0 {
                        let child_bm = select_ith_one(&self.label_bitmap, child_node - 1) + 1;
                        stack.push((child_node, child_bm, path.clone(), true));
                    }
                } else {
                    // Regular character - recurse
                    let child_node = count_zeros(&self.label_bitmap, bm_idx + 1);
                    if child_node > 0 {
                        let child_bm = select_ith_one(&self.label_bitmap, child_node - 1) + 1;
                        let mut child_path = path.clone();
                        child_path.push(lbl);
                        stack.push((child_node, child_bm, child_path, false));
                    }
                }

                bm_idx += 1;
            }
        }

        results
    }
}

impl AdGuardMatcher {
    fn enumerate(&self) -> Vec<(String, String)> {
        // AdGuard trie is similar but more complex; fall back to reporting it as opaque
        let mut results = self.set.enumerate();
        for r in &mut results {
            if r.0 == "domain" {
                r.0 = "domain".to_string();
            }
        }
        results
    }
}

fn block_end_v4(start: u32, prefix_len: u32) -> u32 {
    if prefix_len == 0 {
        u32::MAX
    } else {
        start | ((1u32 << (32 - prefix_len)) - 1)
    }
}

fn range_to_cidrs_v4(from: u32, to: u32) -> Vec<String> {
    let mut results = Vec::new();
    if from > to {
        return results;
    }
    let mut start = from;
    loop {
        // Largest block that starts at `start` is bounded by its alignment,
        // then shrunk until it no longer runs past `to`.
        let mut prefix_len = 32 - if start == 0 { 32 } else { start.trailing_zeros() };
        while prefix_len < 32 && block_end_v4(start, prefix_len) > to {
            prefix_len += 1;
        }
        results.push(format!("{}/{}", Ipv4Addr::from(start), prefix_len));
        let end = block_end_v4(start, prefix_len);
        if end >= to || end == u32::MAX {
            break;
        }
        start = end + 1;
    }
    results
}

fn block_end_v6(start: u128, prefix_len: u32) -> u128 {
    if prefix_len == 0 {
        u128::MAX
    } else {
        start | ((1u128 << (128 - prefix_len)) - 1)
    }
}

fn range_to_cidrs_v6(from: u128, to: u128) -> Vec<String> {
    let mut results = Vec::new();
    if from > to {
        return results;
    }
    let mut start = from;
    loop {
        let mut prefix_len = 128 - if start == 0 { 128 } else { start.trailing_zeros() };
        while prefix_len < 128 && block_end_v6(start, prefix_len) > to {
            prefix_len += 1;
        }
        results.push(format!("{}/{}", Ipv6Addr::from(start), prefix_len));
        let end = block_end_v6(start, prefix_len);
        if end >= to || end == u128::MAX {
            break;
        }
        start = end + 1;
    }
    results
}

impl IpSet {
    fn to_cidrs(&self) -> Vec<String> {
        let mut out = Vec::new();
        for &(f, t) in &self.ranges_v4 {
            out.extend(range_to_cidrs_v4(f, t));
        }
        for &(f, t) in &self.ranges_v6 {
            out.extend(range_to_cidrs_v6(f, t));
        }
        out
    }
}

fn read_u16_list<R: Read>(r: &mut R) -> io::Result<Vec<u16>> {
    let count = read_uvarint(r)? as usize;
    let mut v = Vec::with_capacity(count);
    for _ in 0..count {
        let mut buf = [0u8; 2];
        r.read_exact(&mut buf)?;
        v.push(u16::from_be_bytes(buf));
    }
    Ok(v)
}

fn list_default_rule<R: Read>(r: &mut R) -> io::Result<Vec<RuleEntry>> {
    let mut entries = Vec::new();
    loop {
        let item_type = read_u8(r)?;
        match item_type {
            ITEM_FINAL => {
                let _invert = read_u8(r)?;
                return Ok(entries);
            }
            ITEM_DOMAIN => {
                let set = SuccinctSet::read(r)?;
                for (t, v) in set.enumerate() {
                    entries.push(RuleEntry {
                        rule_type: t,
                        value: v,
                    });
                }
            }
            ITEM_DOMAIN_KEYWORD => {
                for kw in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "domain_keyword".into(),
                        value: kw,
                    });
                }
            }
            ITEM_DOMAIN_REGEX => {
                for rx in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "domain_regex".into(),
                        value: rx,
                    });
                }
            }
            ITEM_IP_CIDR => {
                let set = read_ip_set(r)?;
                for cidr in set.to_cidrs() {
                    entries.push(RuleEntry {
                        rule_type: "ip_cidr".into(),
                        value: cidr,
                    });
                }
            }
            ITEM_SOURCE_IP_CIDR => {
                let set = read_ip_set(r)?;
                for cidr in set.to_cidrs() {
                    entries.push(RuleEntry {
                        rule_type: "source_ip_cidr".into(),
                        value: cidr,
                    });
                }
            }
            ITEM_QUERY_TYPE => {
                for qt in read_u16_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "query_type".into(),
                        value: qt.to_string(),
                    });
                }
            }
            ITEM_NETWORK => {
                for n in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "network".into(),
                        value: n,
                    });
                }
            }
            ITEM_SOURCE_PORT => {
                for p in read_u16_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "source_port".into(),
                        value: p.to_string(),
                    });
                }
            }
            ITEM_PORT => {
                for p in read_u16_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "port".into(),
                        value: p.to_string(),
                    });
                }
            }
            ITEM_SOURCE_PORT_RANGE => {
                for s in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "source_port_range".into(),
                        value: s,
                    });
                }
            }
            ITEM_PORT_RANGE => {
                for s in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "port_range".into(),
                        value: s,
                    });
                }
            }
            ITEM_PROCESS_NAME => {
                for s in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "process_name".into(),
                        value: s,
                    });
                }
            }
            ITEM_PROCESS_PATH => {
                for s in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "process_path".into(),
                        value: s,
                    });
                }
            }
            ITEM_PACKAGE_NAME => {
                for s in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "package_name".into(),
                        value: s,
                    });
                }
            }
            ITEM_WIFI_SSID => {
                for s in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "wifi_ssid".into(),
                        value: s,
                    });
                }
            }
            ITEM_WIFI_BSSID => {
                for s in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "wifi_bssid".into(),
                        value: s,
                    });
                }
            }
            ITEM_PROCESS_PATH_REGEX => {
                for s in read_string_list(r)? {
                    entries.push(RuleEntry {
                        rule_type: "process_path_regex".into(),
                        value: s,
                    });
                }
            }
            ITEM_ADGUARD_DOMAIN => {
                let matcher = AdGuardMatcher::read(r)?;
                for (t, v) in matcher.enumerate() {
                    entries.push(RuleEntry {
                        rule_type: t,
                        value: v,
                    });
                }
            }
            ITEM_NETWORK_TYPE => {
                let count = read_uvarint(r)? as usize;
                for _ in 0..count {
                    let b = read_u8(r)?;
                    entries.push(RuleEntry {
                        rule_type: "network_type".into(),
                        value: b.to_string(),
                    });
                }
            }
            ITEM_NETWORK_IS_EXPENSIVE => {
                entries.push(RuleEntry {
                    rule_type: "network_is_expensive".into(),
                    value: "true".into(),
                });
            }
            ITEM_NETWORK_IS_CONSTRAINED => {
                entries.push(RuleEntry {
                    rule_type: "network_is_constrained".into(),
                    value: "true".into(),
                });
            }
            ITEM_NETWORK_INTERFACE_ADDRESS => {
                let size = read_uvarint(r)? as usize;
                for _ in 0..size {
                    read_u8(r)?;
                    let count = read_uvarint(r)? as usize;
                    for _ in 0..count {
                        let addr_len = read_uvarint(r)? as usize;
                        skip_exact(r, addr_len)?;
                        skip_exact(r, 1)?;
                    }
                }
            }
            ITEM_DEFAULT_INTERFACE_ADDRESS => {
                let count = read_uvarint(r)? as usize;
                for _ in 0..count {
                    let addr_len = read_uvarint(r)? as usize;
                    skip_exact(r, addr_len)?;
                    skip_exact(r, 1)?;
                }
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown item type: {:#x}", other),
                ));
            }
        }
    }
}

fn list_rule<R: Read>(r: &mut R) -> io::Result<Vec<RuleEntry>> {
    match read_u8(r)? {
        0 => list_default_rule(r),
        1 => list_logical_rule(r),
        t => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown rule type: {}", t),
        )),
    }
}

fn list_logical_rule<R: Read>(r: &mut R) -> io::Result<Vec<RuleEntry>> {
    let _mode = read_u8(r)?;
    let count = read_uvarint(r)? as usize;
    let mut entries = Vec::new();
    for _ in 0..count {
        entries.extend(list_rule(r)?);
    }
    let _invert = read_u8(r)?;
    Ok(entries)
}

fn srs_list_bytes(data: &[u8]) -> Result<Vec<RuleEntry>, String> {
    if data.len() < 4 || &data[0..3] != b"SRS" {
        return Err("invalid SRS magic".into());
    }
    let mut decompressed = Vec::new();
    ZlibDecoder::new(&data[4..])
        .read_to_end(&mut decompressed)
        .map_err(|e| format!("decompress: {}", e))?;
    let mut cur = Cursor::new(&decompressed);
    let rule_count =
        read_uvarint(&mut cur).map_err(|e| format!("read rule count: {}", e))? as usize;
    let mut entries = Vec::new();
    for i in 0..rule_count {
        match list_rule(&mut cur) {
            Ok(e) => entries.extend(e),
            Err(e) => return Err(format!("rule[{}]: {}", i, e)),
        }
    }
    Ok(entries)
}

fn resolve_srs_data(
    working_dir: &str,
    config_path: &str,
    singbox_path: &str,
    tag: &str,
) -> Result<Vec<u8>, String> {
    let mut last_cache_error: Option<String> = None;

    if let Some(cache_path) = find_cache_db(&[working_dir, config_path, singbox_path]) {
        let data = std::fs::read(&cache_path)
            .map_err(|e| format!("read '{}': {}", cache_path.to_string_lossy(), e))?;
        let ps = bolt_page_size(&data);
        let root_pgid = bolt_meta_root(&data, ps);

        match (|| -> Result<Vec<u8>, String> {
            let (is_bucket, bval) = bolt_btree_lookup(&data, ps, root_pgid, b"rule_set")
                .map_err(|e| format!("lookup rule_set bucket: {}", e))?
                .ok_or_else(|| "rule_set bucket not found".to_string())?;
            if !is_bucket || bval.len() < 8 {
                return Err("rule_set is not a valid bucket".into());
            }
            let bucket_root = u64::from_le_bytes(bval[0..8].try_into().unwrap());
            let saved = if bucket_root == 0 {
                if bval.len() < 16 + PAGE_HEADER_SIZE {
                    return Err(format!("tag '{}' not found", tag));
                }
                let inline_page = &bval[16..];
                let inline_count = u16::from_le_bytes([inline_page[10], inline_page[11]]) as usize;
                bolt_leaf_lookup(inline_page, inline_count, tag.as_bytes())
                    .map(|(_, v)| v)
                    .ok_or_else(|| format!("tag '{}' not found", tag))?
            } else {
                bolt_btree_lookup(&data, ps, bucket_root, tag.as_bytes())
                    .map_err(|e| format!("lookup '{}': {}", tag, e))?
                    .map(|(_, v)| v)
                    .ok_or_else(|| format!("tag '{}' not found", tag))?
            };
            parse_saved_binary_content(&saved)
                .map_err(|e| format!("parse SavedBinary '{}': {}", tag, e))
        })() {
            Ok(srs_bytes) if srs_bytes.len() >= 4 && &srs_bytes[0..3] == b"SRS" => {
                return Ok(srs_bytes);
            }
            Ok(_) => last_cache_error = Some(format!("cache.db '{}': invalid SRS content", tag)),
            Err(e) => last_cache_error = Some(e),
        }
    }

    if let Some(srs_path) = find_local_srs_from_config(working_dir, config_path, tag) {
        return std::fs::read(&srs_path)
            .map_err(|e| format!("read '{}': {}", srs_path.to_string_lossy(), e));
    }

    if let Some(srs_path) = find_local_srs_by_tag(&[working_dir, config_path, singbox_path], tag) {
        return std::fs::read(&srs_path)
            .map_err(|e| format!("read '{}': {}", srs_path.to_string_lossy(), e));
    }

    if let Some(e) = last_cache_error {
        Err(format!("{}; and local '{}.srs' not found", e, tag))
    } else {
        Err(format!(
            "no cache.db found; and local '{}.srs' not found",
            tag
        ))
    }
}

#[tauri::command]
pub async fn srs_list_provider(
    working_dir: String,
    config_path: String,
    singbox_path: String,
    tag: String,
) -> Result<Vec<RuleEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let data = resolve_srs_data(&working_dir, &config_path, &singbox_path, &tag)?;
        srs_list_bytes(&data)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Match provider by tag:
/// 1) try cache.db if available
/// 2) fallback to local `<tag>.srs` under configured sing-box related directories recursively
#[tauri::command]
pub fn srs_match_provider(
    working_dir: String,
    config_path: String,
    singbox_path: String,
    tag: String,
    query: String,
) -> Result<bool, String> {
    let mut last_cache_error: Option<String> = None;

    if let Some(cache_path) = find_cache_db(&[&working_dir, &config_path, &singbox_path]) {
        match srs_match_cache(
            cache_path.to_string_lossy().into_owned(),
            tag.clone(),
            query.clone(),
        ) {
            Ok(v) => return Ok(v),
            Err(e) => last_cache_error = Some(e),
        }
    }

    if let Some(srs_path) = find_local_srs_from_config(&working_dir, &config_path, &tag) {
        let data = std::fs::read(&srs_path)
            .map_err(|e| format!("read '{}': {}", srs_path.to_string_lossy(), e))?;
        let q = parse_query(&query);
        return srs_match_bytes(&data, &q);
    }

    if let Some(srs_path) =
        find_local_srs_by_tag(&[&working_dir, &config_path, &singbox_path], &tag)
    {
        let data = std::fs::read(&srs_path)
            .map_err(|e| format!("read '{}': {}", srs_path.to_string_lossy(), e))?;
        let q = parse_query(&query);
        return srs_match_bytes(&data, &q);
    }

    if let Some(e) = last_cache_error {
        Err(format!(
            "{}; and local '{}.srs' not found under configured working/config/sing-box directories",
            e, tag
        ))
    } else {
        Err(format!(
            "no cache.db found; and local '{}.srs' not found under configured working/config/sing-box directories",
            tag
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> u32 {
        u32::from_be_bytes(Ipv4Addr::from_str(s).unwrap().octets())
    }

    fn v6(s: &str) -> u128 {
        u128::from_be_bytes(Ipv6Addr::from_str(s).unwrap().octets())
    }

    fn range(q: &str) -> (String, String) {
        match parse_query(q) {
            Query::IpRange(f, t) => (f.to_string(), t.to_string()),
            other => panic!(
                "expected range, got {}",
                match other {
                    Query::Domain(d) => format!("domain({})", d),
                    Query::Ip(ip) => format!("ip({})", ip),
                    Query::IpRange(..) => unreachable!(),
                }
            ),
        }
    }

    #[test]
    fn v4_range_uses_largest_aligned_blocks() {
        assert_eq!(
            range_to_cidrs_v4(v4("1.0.1.240"), v4("1.0.1.255")),
            vec!["1.0.1.240/28"]
        );
        assert_eq!(
            range_to_cidrs_v4(v4("1.0.1.0"), v4("1.0.3.255")),
            vec!["1.0.1.0/24", "1.0.2.0/23"]
        );
        assert_eq!(
            range_to_cidrs_v4(v4("8.8.8.8"), v4("8.8.8.8")),
            vec!["8.8.8.8/32"]
        );
        assert_eq!(
            range_to_cidrs_v4(v4("0.0.0.0"), v4("0.0.0.1")),
            vec!["0.0.0.0/31"]
        );
        assert_eq!(
            range_to_cidrs_v4(0, u32::MAX),
            vec!["0.0.0.0/0"]
        );
    }

    #[test]
    fn v6_range_uses_largest_aligned_blocks() {
        assert_eq!(
            range_to_cidrs_v6(v6("240e:e1:a800::"), v6("240e:e1:a8ff:ffff:ffff:ffff:ffff:ffff")),
            vec!["240e:e1:a800::/40"]
        );
        assert_eq!(
            range_to_cidrs_v6(v6("2001:250::"), v6("2001:256:ffff:ffff:ffff:ffff:ffff:ffff")),
            vec!["2001:250::/30", "2001:254::/31", "2001:256::/32"]
        );
        assert_eq!(range_to_cidrs_v6(0, u128::MAX), vec!["::/0"]);
    }

    #[test]
    fn enumerated_cidrs_cover_the_original_range() {
        let (from, to) = (v4("1.0.1.13"), v4("1.0.7.200"));
        let mut cursor = from;
        for cidr in range_to_cidrs_v4(from, to) {
            let (net, plen) = cidr.split_once('/').unwrap();
            let net = v4(net);
            let plen: u32 = plen.parse().unwrap();
            assert_eq!(net, cursor, "block {} does not continue the range", cidr);
            cursor = block_end_v4(net, plen).wrapping_add(1);
        }
        assert_eq!(cursor, to + 1, "blocks do not cover the whole range");
    }

    #[test]
    fn query_accepts_plain_addresses() {
        assert!(matches!(parse_query("1.0.1.240"), Query::Ip(_)));
        assert!(matches!(parse_query("240e:e1:a800::1"), Query::Ip(_)));
        assert!(matches!(parse_query("example.com"), Query::Domain(_)));
        assert!(matches!(parse_query("163.com"), Query::Domain(_)));
    }

    #[test]
    fn query_accepts_cidr_notation() {
        assert_eq!(
            range("1.0.1.240/29"),
            ("1.0.1.240".into(), "1.0.1.247".into())
        );
        // Host bits outside the prefix are masked off.
        assert_eq!(range("1.0.1.245/29"), ("1.0.1.240".into(), "1.0.1.247".into()));
        assert_eq!(range("0.0.0.0/0"), ("0.0.0.0".into(), "255.255.255.255".into()));
        assert_eq!(
            range("240e:e1:a800::/40"),
            ("240e:e1:a800::".into(), "240e:e1:a8ff:ffff:ffff:ffff:ffff:ffff".into())
        );
    }

    #[test]
    fn query_accepts_truncated_addresses() {
        assert_eq!(
            range("240e:e1:a800"),
            ("240e:e1:a800::".into(), "240e:e1:a800:ffff:ffff:ffff:ffff:ffff".into())
        );
        assert_eq!(
            range("240e:e1:a800:"),
            ("240e:e1:a800::".into(), "240e:e1:a800:ffff:ffff:ffff:ffff:ffff".into())
        );
        assert_eq!(range("1.0.1"), ("1.0.1.0".into(), "1.0.1.255".into()));
        assert_eq!(range("1.0"), ("1.0.0.0".into(), "1.0.255.255".into()));
    }

    #[test]
    fn query_rejects_malformed_ip_like_input() {
        for bad in ["1.0.1.256", "1.0.1.240/33", "240e::e1::a800", "240e:zzzz", "1.0.1.240/x"] {
            assert!(
                matches!(parse_query(bad), Query::Domain(_)),
                "{} should fall back to a domain query",
                bad
            );
        }
    }

    #[test]
    fn ip_set_overlap() {
        let set = IpSet {
            ranges_v4: vec![(v4("1.0.1.0"), v4("1.0.1.255"))],
            ranges_v6: vec![(v6("240e::"), v6("240e:ffff::"))],
        };
        assert!(set.matches(&parse_query("1.0.1.240/29")));
        assert!(set.matches(&parse_query("1.0.1.7")));
        assert!(set.matches(&parse_query("1.0.0.0/8")));
        assert!(!set.matches(&parse_query("1.0.2.0/24")));
        assert!(set.matches(&parse_query("240e:0:1::")));
        // An IPv6 query must never be answered from the IPv4 ranges.
        assert!(!set.matches(&parse_query("2001:250::")));
        assert!(!set.matches(&parse_query("example.com")));
    }
}

