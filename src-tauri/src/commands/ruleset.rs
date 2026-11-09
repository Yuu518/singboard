use serde_json::Value;
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::srs::{self, Query, RuleEntry};
use crate::root::{self, fs, quote};

const SCAN_DEPTH: usize = 6;
const SCAN_LIMIT: usize = 4000;
const CACHE_TTL: Duration = Duration::from_secs(15);
const TAG_SUFFIXES: [&str; 6] = ["-domain", "-ip", "-filter", "-cidr", "-geoip", "-geosite"];

pub(crate) enum RuleSet {
    Binary(Vec<u8>),
    Source(Value),
}

impl RuleSet {
    pub(crate) fn list(&self) -> Result<Vec<RuleEntry>, String> {
        match self {
            RuleSet::Binary(data) => srs::srs_list_bytes(data),
            RuleSet::Source(value) => {
                let mut entries = Vec::new();
                for rule in source_rules(value) {
                    collect_source_rule(rule, &mut entries);
                }
                Ok(entries)
            }
        }
    }

    pub(crate) fn matches(&self, query: &Query) -> Result<bool, String> {
        match self {
            RuleSet::Binary(data) => srs::srs_match_bytes(data, query),
            RuleSet::Source(value) => Ok(source_rules(value)
                .iter()
                .any(|rule| source_rule_matches(rule, query))),
        }
    }
}

fn strip_comments(text: &str) -> String {
    let chars: Vec<char> = text.trim_start_matches('\u{feff}').chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(&next) = chars.get(i + 1) {
                    out.push(next);
                    i += 1;
                }
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match (c, chars.get(i + 1)) {
            ('"', _) => {
                in_string = true;
                out.push(c);
                i += 1;
            }
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
                out.push(' ');
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(&next) = chars.get(i + 1) {
                    out.push(next);
                    i += 1;
                }
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

pub(crate) fn parse_jsonc(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).or_else(|_| {
        serde_json::from_str(&strip_trailing_commas(&strip_comments(text)))
            .map_err(|e| format!("JSON 解析失败: {e}"))
    })
}

fn decode(data: Vec<u8>) -> Result<RuleSet, String> {
    if data.starts_with(b"SRS") {
        return Ok(RuleSet::Binary(data));
    }
    let text = std::str::from_utf8(&data).map_err(|_| "不是 SRS 或 JSON 规则集".to_string())?;
    let value = parse_jsonc(text)?;
    if value.get("rules").is_some_and(Value::is_array) {
        Ok(RuleSet::Source(value))
    } else {
        Err("JSON 中没有 rules 数组，不是规则集".into())
    }
}

fn source_rules(value: &Value) -> &[Value] {
    value
        .get("rules")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn is_logical(rule: &Value) -> bool {
    rule.get("type").and_then(Value::as_str) == Some("logical")
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn listable(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(scalar).collect(),
        Some(other) => scalar(other).into_iter().collect(),
        None => Vec::new(),
    }
}

fn collect_source_rule(rule: &Value, entries: &mut Vec<RuleEntry>) {
    if is_logical(rule) {
        for inner in source_rules(rule) {
            collect_source_rule(inner, entries);
        }
        return;
    }
    let Some(fields) = rule.as_object() else {
        return;
    };
    for (key, value) in fields {
        if key == "invert" || key == "type" {
            continue;
        }
        for item in listable(Some(value)) {
            entries.push(RuleEntry {
                rule_type: key.clone(),
                value: item,
            });
        }
    }
}

fn suffix_matches(domain: &str, suffix: &str) -> bool {
    let suffix = suffix.to_ascii_lowercase();
    if suffix.starts_with('.') {
        domain.ends_with(&suffix)
    } else {
        domain == suffix || domain.ends_with(&format!(".{suffix}"))
    }
}

fn same_family(a: &IpAddr, b: &IpAddr) -> bool {
    a.is_ipv4() == b.is_ipv4()
}

fn cidr_range(value: &str) -> Option<(IpAddr, IpAddr)> {
    let value = value.trim();
    srs::parse_cidr(value).or_else(|| IpAddr::from_str(value).ok().map(|ip| (ip, ip)))
}

fn cidr_matches(value: &str, query: &Query) -> bool {
    let Some((from, to)) = cidr_range(value) else {
        return false;
    };
    match query {
        Query::Ip(ip) => same_family(ip, &from) && from <= *ip && *ip <= to,
        Query::IpRange(lo, hi) => same_family(lo, &from) && *lo <= to && *hi >= from,
        Query::Domain(_) => false,
    }
}

fn source_rule_matches(rule: &Value, query: &Query) -> bool {
    let result = if is_logical(rule) {
        let rules = source_rules(rule);
        let all = rule
            .get("mode")
            .and_then(Value::as_str)
            .is_some_and(|mode| mode.eq_ignore_ascii_case("and"));
        if all {
            !rules.is_empty() && rules.iter().all(|r| source_rule_matches(r, query))
        } else {
            rules.iter().any(|r| source_rule_matches(r, query))
        }
    } else {
        match query {
            Query::Domain(domain) => {
                let exact = listable(rule.get("domain"));
                let suffix = listable(rule.get("domain_suffix"));
                let keyword = listable(rule.get("domain_keyword"));
                exact.iter().any(|d| d.eq_ignore_ascii_case(domain))
                    || suffix.iter().any(|s| suffix_matches(domain, s))
                    || keyword.iter().any(|k| domain.contains(&k.to_ascii_lowercase()))
            }
            Query::Ip(_) | Query::IpRange(..) => listable(rule.get("ip_cidr"))
                .iter()
                .any(|c| cidr_matches(c, query)),
        }
    };
    if rule.get("invert").and_then(Value::as_bool).unwrap_or(false) {
        !result
    } else {
        result
    }
}

fn load_config(config_path: &str) -> Option<Value> {
    if config_path.trim().is_empty() {
        return None;
    }
    let text = fs::read_to_string(Path::new(config_path)).ok()?;
    parse_jsonc(&text).ok()
}

fn config_rule_set<'a>(config: &'a Value, tag: &str) -> Option<&'a Value> {
    config
        .get("route")?
        .get("rule_set")?
        .as_array()?
        .iter()
        .find(|item| item.get("tag").and_then(Value::as_str) == Some(tag))
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || Path::new(path).is_absolute()
}

fn resolve_against(base: &str, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    let joined = if is_absolute(raw) || base.is_empty() {
        path.to_path_buf()
    } else {
        Path::new(base).join(path)
    };
    joined.components().collect()
}

fn configured_cache(config: &Value, working_dir: &str) -> Option<PathBuf> {
    let cache = config.get("experimental")?.get("cache_file")?;
    let path = cache
        .get("path")
        .and_then(Value::as_str)
        .filter(|p| !p.trim().is_empty())
        .unwrap_or("cache.db");
    Some(resolve_against(working_dir, path))
}

fn configured_cache_id(config: &Value) -> Option<String> {
    config
        .get("experimental")?
        .get("cache_file")?
        .get("cache_id")?
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn cache_file_enabled(config: &Value) -> bool {
    config
        .get("experimental")
        .and_then(|e| e.get("cache_file"))
        .and_then(|c| c.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn local_path_candidates(raw: &str, working_dir: &str, config_path: &str) -> Vec<PathBuf> {
    let mut bases = vec![working_dir.to_string()];
    if let Some(parent) = Path::new(config_path).parent() {
        bases.push(parent.to_string_lossy().into_owned());
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for base in bases {
        let path = resolve_against(&base, raw);
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

fn scan_roots(working_dir: &str, config_path: &str, singbox_path: &str) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut add = |path: PathBuf| {
        if !is_absolute(&path.to_string_lossy()) {
            return;
        }
        if roots.iter().any(|root| path.starts_with(root)) {
            return;
        }
        roots.retain(|root| !root.starts_with(&path));
        roots.push(path);
    };
    if !working_dir.trim().is_empty() {
        add(PathBuf::from(working_dir.trim()));
    }
    for file in [config_path, singbox_path] {
        if let Some(parent) = Path::new(file.trim()).parent() {
            add(parent.to_path_buf());
        }
    }
    roots
}

fn wanted_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["srs", "json", "db"].iter().any(|w| e.eq_ignore_ascii_case(w)))
}

fn find_files_root(root: &Path) -> Vec<PathBuf> {
    let script = format!(
        "find {} -maxdepth {SCAN_DEPTH} -type f \\( -iname '*.srs' -o -iname '*.json' -o -iname '*.db' \\) 2>/dev/null | head -n {SCAN_LIMIT}",
        quote(&root.to_string_lossy())
    );
    root::run(&script)
        .map(|output| output.lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect())
        .unwrap_or_default()
}

fn find_files_local(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth + 1 < SCAN_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if kind.is_file() && wanted_extension(&path) {
                out.push(path);
                if out.len() >= SCAN_LIMIT {
                    return out;
                }
            }
        }
    }
    out
}

struct Scan {
    key: Vec<PathBuf>,
    at: Instant,
    files: Arc<Vec<PathBuf>>,
}

static SCAN: Mutex<Option<Scan>> = Mutex::new(None);

fn scan_files(roots: &[PathBuf]) -> Arc<Vec<PathBuf>> {
    let mut guard = match SCAN.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(scan) = guard.as_ref() {
        if scan.key == roots && scan.at.elapsed() < CACHE_TTL {
            return scan.files.clone();
        }
    }
    let mut files = Vec::new();
    for root in roots {
        if root::enabled() {
            files.extend(find_files_root(root));
        } else {
            files.extend(find_files_local(root));
        }
    }
    let files = Arc::new(files);
    *guard = Some(Scan {
        key: roots.to_vec(),
        at: Instant::now(),
        files: files.clone(),
    });
    files
}

fn tag_stems(tag: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim().to_ascii_lowercase();
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    push(tag);
    for suffix in TAG_SUFFIXES {
        if let Some(stripped) = tag.strip_suffix(suffix) {
            push(stripped);
        }
    }
    if let Some((head, _)) = tag.rsplit_once('-') {
        push(head);
    }
    out
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn depth_of(path: &Path) -> usize {
    path.components().count()
}

fn local_candidates(files: &[PathBuf], tag: &str) -> Vec<PathBuf> {
    let stems = tag_stems(tag);
    let mut found: Vec<(usize, bool, usize, PathBuf)> = files
        .iter()
        .filter_map(|path| {
            let ext = extension_of(path);
            if ext != "srs" && ext != "json" {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
            let rank = stems.iter().position(|s| *s == stem)?;
            Some((rank, ext != "srs", depth_of(path), path.clone()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(.., path)| path).collect()
}

fn db_candidates(files: &[PathBuf], preferred: Option<PathBuf>) -> Vec<PathBuf> {
    let mut found: Vec<(bool, usize, PathBuf)> = files
        .iter()
        .filter(|path| extension_of(path) == "db")
        .map(|path| {
            let is_cache = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case("cache.db"));
            (!is_cache, depth_of(path), path.clone())
        })
        .collect();
    found.sort();
    let mut out: Vec<PathBuf> = preferred.into_iter().collect();
    for (.., path) in found {
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

fn locate(working_dir: &str, config_path: &str, singbox_path: &str, tag: &str) -> Result<RuleSet, String> {
    let config = load_config(config_path);
    let mut problems: Vec<String> = Vec::new();

    if let Some(entry) = config.as_ref().and_then(|c| config_rule_set(c, tag)) {
        let kind = entry.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "inline" || (kind.is_empty() && entry.get("rules").is_some()) {
            return Ok(RuleSet::Source(entry.clone()));
        }
        if kind == "remote" && !config.as_ref().is_some_and(cache_file_enabled) {
            problems.push(format!(
                "{tag} 是远程规则集，但配置未启用 experimental.cache_file，sing-box 不会把它保存到磁盘"
            ));
        }
        if kind == "local" {
            if let Some(raw) = entry.get("path").and_then(Value::as_str).filter(|p| !p.trim().is_empty()) {
                for path in local_path_candidates(raw, working_dir, config_path) {
                    if !fs::is_file(&path) {
                        continue;
                    }
                    match fs::read(&path).and_then(decode) {
                        Ok(set) => return Ok(set),
                        Err(e) => problems.push(format!("{}: {e}", path.display())),
                    }
                }
            }
        }
    }

    let files = scan_files(&scan_roots(working_dir, config_path, singbox_path));

    for path in local_candidates(&files, tag) {
        match fs::read(&path).and_then(decode) {
            Ok(set) => return Ok(set),
            Err(e) => problems.push(format!("{}: {e}", path.display())),
        }
    }

    let preferred = config.as_ref().and_then(|c| configured_cache(c, working_dir));
    let cache_id = config.as_ref().and_then(configured_cache_id);
    for db in db_candidates(&files, preferred.clone()) {
        let data = match fs::read_shared(&db) {
            Ok(data) => data,
            Err(e) => {
                if preferred.as_ref() == Some(&db) {
                    problems.push(e);
                }
                continue;
            }
        };
        match srs::cached_rule_set(&data, tag, cache_id.as_deref()) {
            Ok(content) => match decode(content) {
                Ok(set) => return Ok(set),
                Err(e) => problems.push(format!("{}: {e}", db.display())),
            },
            Err(e) if e == srs::NOT_BOLT => continue,
            Err(e) => {
                let is_cache = preferred.as_ref() == Some(&db)
                    || db.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case("cache.db"));
                if is_cache {
                    problems.push(format!("{}: {e}", db.display()));
                }
            }
        }
    }

    let mut message = format!(
        "未找到规则集 {tag}：工作目录及其子目录中没有对应的 .srs / .json 文件，.db 缓存数据库中也没有该规则集"
    );
    for problem in problems.iter().take(3) {
        message.push('\n');
        message.push_str(problem);
    }
    Err(message)
}

type Resolved = HashMap<String, (Instant, Arc<RuleSet>)>;

static RESOLVED: Mutex<Option<Resolved>> = Mutex::new(None);

pub(crate) fn resolve(
    working_dir: &str,
    config_path: &str,
    singbox_path: &str,
    tag: &str,
) -> Result<Arc<RuleSet>, String> {
    let key = format!("{working_dir}\n{config_path}\n{singbox_path}\n{tag}");
    if let Ok(guard) = RESOLVED.lock() {
        if let Some((at, set)) = guard.as_ref().and_then(|map| map.get(&key)) {
            if at.elapsed() < CACHE_TTL {
                return Ok(set.clone());
            }
        }
    }
    let set = Arc::new(locate(working_dir, config_path, singbox_path, tag)?);
    if let Ok(mut guard) = RESOLVED.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        map.insert(key, (Instant::now(), set.clone()));
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source(value: Value) -> RuleSet {
        RuleSet::Source(value)
    }

    fn matches(set: &RuleSet, query: &str) -> bool {
        set.matches(&srs::parse_query(query)).unwrap()
    }

    #[test]
    fn jsonc_comments_and_trailing_commas_are_accepted() {
        let text = "\u{feff}{\n  // comment\n  \"url\": \"http://a/b\", /* block */\n  \"rules\": [\"x\",],\n}";
        let value = parse_jsonc(text).unwrap();
        assert_eq!(value["url"], "http://a/b");
        assert_eq!(value["rules"], json!(["x"]));
    }

    #[test]
    fn decode_distinguishes_binary_source_and_other_json() {
        assert!(matches!(decode(b"SRS\x03rest".to_vec()), Ok(RuleSet::Binary(_))));
        assert!(matches!(
            decode(br#"{"version":3,"rules":[{"domain":"a.com"}]}"#.to_vec()),
            Ok(RuleSet::Source(_))
        ));
        assert!(decode(br#"{"outbounds":[]}"#.to_vec()).is_err());
        assert!(decode(vec![0xff, 0xfe, 0x00]).is_err());
    }

    #[test]
    fn source_rule_set_matches_domains_like_sing_box() {
        let set = source(json!({
            "version": 3,
            "rules": [
                { "domain": ["exact.com"], "domain_suffix": ["google.com", ".cn"], "domain_keyword": "tracker" }
            ]
        }));
        assert!(matches(&set, "exact.com"));
        assert!(!matches(&set, "sub.exact.com"));
        assert!(matches(&set, "google.com"));
        assert!(matches(&set, "www.google.com"));
        assert!(!matches(&set, "notgoogle.com"));
        assert!(matches(&set, "baidu.cn"));
        assert!(!matches(&set, "cn"));
        assert!(matches(&set, "ads-tracker.net"));
        assert!(!matches(&set, "1.1.1.1"));
    }

    #[test]
    fn source_rule_set_matches_ip_cidr_and_ranges() {
        let set = source(json!({ "rules": [{ "ip_cidr": ["10.0.0.0/8", "2400:3200::/32", "1.1.1.1"] }] }));
        assert!(matches(&set, "10.1.2.3"));
        assert!(matches(&set, "1.1.1.1"));
        assert!(!matches(&set, "1.1.1.2"));
        assert!(matches(&set, "2400:3200::1"));
        assert!(matches(&set, "10.1"));
        assert!(!matches(&set, "11.0.0.0/8"));
        assert!(!matches(&set, "example.com"));
    }

    #[test]
    fn logical_and_inverted_source_rules() {
        let set = source(json!({
            "rules": [
                { "type": "logical", "mode": "and", "rules": [
                    { "domain_suffix": "example.com" },
                    { "domain_keyword": "cdn" }
                ]},
                { "domain_suffix": "blocked.org", "invert": true }
            ]
        }));
        assert!(matches(&set, "cdn.example.com"));
        assert!(matches(&set, "any.net"));
        assert!(!matches(&set, "www.blocked.org"));
    }

    #[test]
    fn source_rule_set_lists_every_value() {
        let set = source(json!({
            "rules": [
                { "domain_suffix": ["a.com", "b.com"], "ip_cidr": "1.0.0.0/8", "network_is_expensive": true },
                { "type": "logical", "mode": "or", "rules": [{ "package_name": "com.app" }] }
            ]
        }));
        let entries: Vec<(String, String)> = set
            .list()
            .unwrap()
            .into_iter()
            .map(|e| (e.rule_type, e.value))
            .collect();
        assert!(entries.contains(&("domain_suffix".into(), "a.com".into())));
        assert!(entries.contains(&("domain_suffix".into(), "b.com".into())));
        assert!(entries.contains(&("ip_cidr".into(), "1.0.0.0/8".into())));
        assert!(entries.contains(&("network_is_expensive".into(), "true".into())));
        assert!(entries.contains(&("package_name".into(), "com.app".into())));
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn local_files_prefer_exact_tag_srs_and_shallow_paths() {
        let files: Vec<PathBuf> = [
            "/w/rules/deep/geosite-cn.srs",
            "/w/rules/geosite-cn.json",
            "/w/rules/geosite-cn.srs",
            "/w/rules/geosite.srs",
            "/w/config.json",
            "/w/cache.db",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let found = local_candidates(&files, "geosite-cn");
        assert_eq!(
            found,
            vec![
                PathBuf::from("/w/rules/geosite-cn.srs"),
                PathBuf::from("/w/rules/deep/geosite-cn.srs"),
                PathBuf::from("/w/rules/geosite-cn.json"),
                PathBuf::from("/w/rules/geosite.srs"),
            ]
        );
    }

    #[test]
    fn databases_try_configured_cache_then_cache_db_then_others() {
        let files: Vec<PathBuf> = ["/w/data/other.db", "/w/run/deep/cache.db", "/w/geo.db"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let found = db_candidates(&files, Some(PathBuf::from("/w/custom.db")));
        assert_eq!(
            found,
            vec![
                PathBuf::from("/w/custom.db"),
                PathBuf::from("/w/run/deep/cache.db"),
                PathBuf::from("/w/geo.db"),
                PathBuf::from("/w/data/other.db"),
            ]
        );
    }

    #[test]
    fn scan_roots_collapse_nested_directories() {
        let roots = scan_roots("/data/adb/box", "/data/adb/box/conf/config.json", "/data/adb/bin/sing-box");
        assert_eq!(roots, vec![PathBuf::from("/data/adb/box"), PathBuf::from("/data/adb/bin")]);
    }

    #[test]
    fn config_cache_path_is_relative_to_the_working_directory() {
        let config = json!({ "experimental": { "cache_file": { "enabled": true, "path": "run/cache.db" } } });
        assert_eq!(configured_cache(&config, "/w"), Some(PathBuf::from("/w/run/cache.db")));
        let dotted = json!({ "experimental": { "cache_file": { "enabled": true, "path": "./run/cache.db" } } });
        let shown = configured_cache(&dotted, "/w").unwrap().to_string_lossy().into_owned();
        assert!(shown.split(['/', '\\']).all(|segment| segment != "."), "{shown}");
        let default = json!({ "experimental": { "cache_file": { "enabled": true } } });
        assert_eq!(configured_cache(&default, "/w"), Some(PathBuf::from("/w/cache.db")));
        assert_eq!(configured_cache(&json!({}), "/w"), None);
    }

    #[test]
    fn non_bolt_databases_are_rejected_without_panicking() {
        assert_eq!(srs::cached_rule_set(b"SQLite format 3\0", "x", None).unwrap_err(), srs::NOT_BOLT);
        let mut garbage = vec![0u8; 8192];
        garbage[16..20].copy_from_slice(&0xED0C_DAEDu32.to_le_bytes());
        garbage[24..28].copy_from_slice(&4096u32.to_le_bytes());
        garbage[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(srs::cached_rule_set(&garbage, "x", None).is_err());
        assert!(srs::cached_rule_set(&garbage, "x", Some("id")).is_err());
    }

    #[test]
    fn config_cache_id_and_enabled_are_read() {
        let config = json!({ "experimental": { "cache_file": { "enabled": true, "cache_id": "phone" } } });
        assert_eq!(configured_cache_id(&config), Some("phone".into()));
        assert!(cache_file_enabled(&config));
        let blank = json!({ "experimental": { "cache_file": { "cache_id": "" } } });
        assert_eq!(configured_cache_id(&blank), None);
        assert!(!cache_file_enabled(&blank));
        assert!(!cache_file_enabled(&json!({})));
    }
}
