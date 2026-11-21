use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
pub const LEVELS: [&str; 8] = [
    "跟随核心",
    "panic",
    "fatal",
    "error",
    "warn",
    "info",
    "debug",
    "trace",
];
pub const SORTS: [(&str, &str); 7] = [
    ("recent", "最新"),
    ("duration", "时长最长"),
    ("download", "下载速度"),
    ("upload", "上传速度"),
    ("downloadTotal", "下载总量"),
    ("uploadTotal", "上传总量"),
    ("host", "域名"),
];
pub fn level_rank(level: &str) -> usize {
    if level == "warning" {
        return 4;
    }
    LEVELS.iter().position(|v| *v == level).unwrap_or(5)
}
pub fn host(row: &Value) -> &str {
    ["host", "sniffHost", "destinationIP"]
        .into_iter()
        .filter_map(|k| row["metadata"][k].as_str())
        .find(|s| !s.is_empty())
        .unwrap_or("未知目标")
}
pub fn compare(a: &Value, b: &Value, key: &str, closed: bool) -> std::cmp::Ordering {
    let start = |r: &Value| r["startMs"].as_i64().unwrap_or_default();
    let numeric = |r: &Value| match key {
        "duration" => {
            let s = start(r);
            if s <= 0 {
                i64::MIN
            } else if closed {
                r["closedAt"].as_i64().unwrap_or(s).saturating_sub(s)
            } else {
                -s
            }
        }
        "upload" if !closed => r["uploadSpeed"].as_i64().unwrap_or(0),
        "download" if !closed => r["downloadSpeed"].as_i64().unwrap_or(0),
        "uploadTotal" => r["upload"].as_i64().unwrap_or(0),
        "downloadTotal" => r["download"].as_i64().unwrap_or(0),
        _ if closed => r["closedAt"].as_i64().unwrap_or(0),
        _ => start(r),
    };
    (if key == "host" {
        host(a).cmp(host(b))
    } else {
        numeric(b).cmp(&numeric(a))
    })
    .then_with(|| start(b).cmp(&start(a)))
    .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
}
pub fn resolve_exit(proxies: &Value, mode: &str) -> Value {
    let normalized = mode.trim().to_ascii_lowercase();
    if normalized == "direct" {
        return json!({"node":"DIRECT","via":[],"direct":true,"latency":Value::Null});
    }
    let map = &proxies["proxies"];
    let Some(all) = map.as_object() else {
        return Value::Null;
    };
    let is_selector = |name: &str| {
        map.get(name).is_some_and(|p| {
            p["type"]
                .as_str()
                .is_some_and(|t| t.eq_ignore_ascii_case("selector"))
                && p["all"].as_array().is_some_and(|a| !a.is_empty())
        })
    };
    let start = if normalized == "global" && map.get("GLOBAL").is_some() {
        Some("GLOBAL")
    } else if let Some(order) = map["GLOBAL"]["all"].as_array() {
        order
            .iter()
            .filter_map(Value::as_str)
            .find(|&name| name != "GLOBAL" && is_selector(name))
            .or_else(|| {
                all.keys()
                    .map(String::as_str)
                    .find(|&name| name != "GLOBAL" && is_selector(name))
            })
    } else {
        all.keys()
            .map(String::as_str)
            .find(|&name| name != "GLOBAL" && is_selector(name))
    };
    let Some(start) = start else {
        return Value::Null;
    };
    let path = chain_list(map, start);
    let Some(&node) = path.last() else {
        return Value::Null;
    };
    if node == start && map[start]["all"].as_array().is_some_and(|a| !a.is_empty()) {
        return Value::Null;
    }
    let via: Vec<&str> = path[..path.len() - 1]
        .iter()
        .copied()
        .filter(|&name| name != "GLOBAL")
        .collect();
    let direct = node.eq_ignore_ascii_case("DIRECT");
    let latency = if direct {
        Value::Null
    } else {
        map[node]["history"]
            .as_array()
            .and_then(|h| h.last())
            .and_then(|v| v["delay"].as_u64())
            .map(Value::from)
            .unwrap_or(Value::Null)
    };
    json!({"node":node,"via":via,"direct":direct,"latency":latency})
}

pub fn route_summary(proxies: &Value) -> String {
    let map = &proxies["proxies"];
    let Some(all) = map.as_object() else {
        return String::new();
    };
    let name = map["GLOBAL"]["now"]
        .as_str()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            all.iter()
                .find(|(_, p)| p["now"].as_str().is_some_and(|v| !v.is_empty()))
                .map(|(k, _)| k.as_str())
        })
        .unwrap_or("");
    chain(map, name, true)
}
pub fn chain_list<'a>(map: &'a Value, mut name: &'a str) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    let mut list = Vec::new();
    while !name.is_empty() && seen.insert(name) {
        list.push(name);
        if let Some(next) = map[name]["now"].as_str().filter(|v| !v.is_empty()) {
            name = next;
        } else {
            break;
        }
    }
    list
}
pub fn chain<'a>(map: &'a Value, name: &'a str, latency: bool) -> String {
    let list = chain_list(map, name);
    let last = list.last().copied().unwrap_or("");
    let mut result = list.join(" → ");
    if latency
        && let Some(ms) = map[last]["history"]
            .as_array()
            .and_then(|h| h.last())
            .and_then(|v| v["delay"].as_u64())
    {
        result.push_str(&format!(" · {ms} ms"));
    }
    result
}
pub fn metrics(snapshot: &Value) -> Value {
    let mut tcp = 0;
    let mut udp = 0;
    let mut rules = BTreeMap::<String, usize>::new();
    let mut outbounds = BTreeMap::<String, usize>::new();
    let mut chain_speeds = BTreeMap::<String, [u64; 2]>::new();
    for row in snapshot["connections"].as_array().into_iter().flatten() {
        let dl_spd = row["downloadSpeed"].as_u64().unwrap_or(0);
        let ul_spd = row["uploadSpeed"].as_u64().unwrap_or(0);
        if dl_spd > 0 || ul_spd > 0 {
            let mut seen = HashSet::new();
            for name in row["chains"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if seen.insert(name) {
                    let entry = chain_speeds.entry(name.to_owned()).or_default();
                    entry[0] = entry[0].saturating_add(dl_spd);
                    entry[1] = entry[1].saturating_add(ul_spd);
                }
            }
        }
        match row["metadata"]["network"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "tcp" => tcp += 1,
            "udp" => udp += 1,
            _ => {}
        }
        *rules
            .entry(
                row["rule"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .unwrap_or("默认")
                    .to_owned(),
            )
            .or_default() += 1;
        let chain = row["chains"]
            .as_array()
            .into_iter()
            .flatten()
            .rev()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" → ");
        *outbounds
            .entry(if chain.is_empty() {
                "默认".into()
            } else {
                chain
            })
            .or_default() += 1;
    }
    json!({"connectionCount":snapshot["connections"].as_array().map_or(0, Vec::len),"flow":flow(snapshot),"tcp":tcp,"udp":udp,"rules":rules,"outbounds":outbounds,"chainSpeeds":chain_speeds,"download":snapshot["downloadTotal"],"upload":snapshot["uploadTotal"],"memory":snapshot["memory"]})
}
fn flow(snapshot: &Value) -> Value {
    let mut paths = Vec::new();
    let mut counts: [BTreeMap<String, usize>; 4] = Default::default();
    for row in snapshot["connections"].as_array().into_iter().flatten() {
        let Some(chain) = row["chains"].as_array() else {
            continue;
        };
        let Some(exit) = chain.first().and_then(Value::as_str) else {
            continue;
        };
        let entry = chain.last().and_then(Value::as_str).unwrap_or(exit);
        let rule_name = row["rule"].as_str().unwrap_or("");
        let rule_payload = row["rulePayload"].as_str().unwrap_or("");
        let rule = if !rule_payload.is_empty() && !rule_name.is_empty() {
            format!("{rule_name}: {rule_payload}")
        } else if !rule_name.is_empty() {
            rule_name.to_owned()
        } else if !rule_payload.is_empty() {
            rule_payload.to_owned()
        } else {
            "未知规则".to_owned()
        };
        let mut path = vec![
            (
                0,
                row["metadata"]["sourceIP"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("未知来源")
                    .to_owned(),
            ),
            (1, rule),
        ];
        if entry != exit {
            path.push((2, entry.into()));
        }
        path.push((3, exit.into()));
        for (depth, label) in &path {
            *counts[*depth].entry(label.clone()).or_default() += 1;
        }
        paths.push(path);
    }
    let connection_count = paths.len();
    let mut nodes = Vec::new();
    let mut locations = BTreeMap::new();
    let mut sizes = [0usize; 4];
    for (depth, counts) in counts.into_iter().enumerate() {
        let mut ranked = counts.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let keep_limit = if ranked.len() > 8 { 7 } else { ranked.len() };
        let tail = ranked.split_off(keep_limit);
        ranked.sort_by(|a, b| a.0.cmp(&b.0));
        let merged_count = tail.len();
        let other = nodes.len() + ranked.len();
        if !tail.is_empty() {
            ranked.push((
                format!("其他 {merged_count} 个"),
                tail.iter().map(|p| p.1).sum(),
            ));
        }
        let n_nodes = ranked.len();
        sizes[depth] = n_nodes;
        for (idx, (name, count)) in ranked.into_iter().enumerate() {
            let merged = if !tail.is_empty() && idx + 1 == n_nodes {
                merged_count
            } else {
                0
            };
            if merged == 0 {
                locations.insert((depth, name.clone()), nodes.len());
            }
            nodes.push(json!({"layer":depth,"name":name,"count":count,"merged":merged}));
        }
        for (name, _) in tail {
            locations.insert((depth, name), other);
        }
    }
    let mut links = BTreeMap::<(usize, usize), usize>::new();
    for path in paths {
        for pair in path.windows(2) {
            *links
                .entry((locations[&pair[0]], locations[&pair[1]]))
                .or_default() += 1;
        }
    }
    let mut weights = vec![(0_f64, 0_f64); nodes.len()];
    let links = links.into_iter().map(|((a, b), count)| {
        let weight = (count as f64 + 1.).log10() * 10.;
        weights[a].0 += weight;
        weights[b].1 += weight;
        (a, b, count, weight)
    }).collect::<Vec<_>>();
    let height = (sizes.iter().max().copied().unwrap_or(0) as f64 * 28. + 24.).clamp(160., 320.);
    let mut sums = [0_f64; 4];
    for (node, (outgoing, incoming)) in nodes.iter().zip(&weights) {
        sums[node["layer"].as_u64().unwrap() as usize] += outgoing.max(*incoming);
    }
    let scale = sums.iter().zip(sizes).filter(|(sum, _)| **sum > 0.)
        .map(|(sum, n)| (height - 16. - 12. * n.saturating_sub(1) as f64) / sum / height)
        .fold(f64::INFINITY, f64::min);
    let mut cursor = [8. / height; 4];
    for (node, (outgoing, incoming)) in nodes.iter_mut().zip(weights) {
        let layer = node["layer"].as_u64().unwrap() as usize;
        let h = outgoing.max(incoming) * scale;
        node["y"] = cursor[layer].into();
        node["h"] = h.into();
        cursor[layer] += h + 12. / height;
    }
    let mut offsets = vec![(0., 0.); nodes.len()];
    let mut edges = Vec::with_capacity(links.len());
    for (a, b, count, weight) in links {
        let w = weight * scale;
        let start = nodes[a]["y"].as_f64().unwrap() + offsets[a].0 + w / 2.;
        let end = nodes[b]["y"].as_f64().unwrap() + offsets[b].1 + w / 2.;
        offsets[a].0 += w;
        offsets[b].1 += w;
        edges.push(json!([nodes[a]["layer"], start, nodes[b]["layer"], end, w, a, b, count]));
    }
    json!({"nodes":nodes,"edges":edges,"connectionCount":connection_count})
}
pub fn spark(samples: impl Iterator<Item = u64>) -> String {
    let items = samples.collect::<Vec<_>>();
    let lo = items.iter().min().copied().unwrap_or(0);
    let span = items.iter().max().copied().unwrap_or(lo) - lo;
    items
        .into_iter()
        .map(|v| {
            ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█']
                [((v - lo).saturating_mul(7) / span.max(1)) as usize]
        })
        .collect()
}
