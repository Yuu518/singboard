use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::root::{self, quote};

const SCAN_DEPTH: u32 = 4;
const MAX_CONFIG_CANDIDATES: usize = 64;
const RULE_DIR_NAMES: [&str; 8] = [
    "rules", "rule", "ruleset", "rulesets", "rule-set", "rule-sets", "rule_set", "rule_sets",
];
const NON_BINARY_EXTENSIONS: [&str; 12] = [
    "json", "db", "srs", "log", "txt", "gz", "zip", "tar", "sh", "yaml", "yml", "bak",
];

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectedRuntimeFiles {
    pub base_dir: String,
    pub singbox_path: Option<String>,
    pub core_install_path: String,
    pub config_path: Option<String>,
    pub rules_dir: Option<String>,
    pub found: bool,
}

fn depth(path: &str) -> usize {
    path.matches('/').count()
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(".")
}

fn extension(path: &str) -> Option<String> {
    let name = file_name(path);
    name.rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

fn in_bin_dir(path: &str) -> bool {
    file_name(parent(path)) == "bin"
}

fn named_like_core(path: &str) -> bool {
    let name = file_name(path).to_ascii_lowercase();
    name.starts_with("sing-box") || name.starts_with("singbox")
}

fn core_candidate(path: &str) -> bool {
    let binary_like = extension(path)
        .map(|ext| !NON_BINARY_EXTENSIONS.contains(&ext.as_str()))
        .unwrap_or(true);
    binary_like && (in_bin_dir(path) || named_like_core(path))
}

fn config_candidate(path: &str) -> bool {
    extension(path).as_deref() == Some("json")
}

fn shallowest<'a>(items: impl Iterator<Item = &'a String>) -> Option<&'a String> {
    items.min_by(|a, b| depth(a).cmp(&depth(b)).then_with(|| a.cmp(b)))
}

fn join(base: &str, relative: &str) -> String {
    let relative = relative.trim_start_matches("./");
    if relative.is_empty() || relative == "." {
        base.to_string()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), relative)
    }
}

fn pick(
    base: &str,
    dirs: &[String],
    files: &[String],
    elf: &HashSet<String>,
    configs: &HashSet<String>,
) -> DetectedRuntimeFiles {
    let core = files
        .iter()
        .filter(|f| elf.contains(*f))
        .min_by_key(|f| {
            (
                !in_bin_dir(f),
                !matches!(file_name(f).to_ascii_lowercase().as_str(), "sing-box" | "singbox"),
                !named_like_core(f),
                depth(f),
                (*f).clone(),
            )
        });

    let config = files
        .iter()
        .filter(|f| configs.contains(*f) || file_name(f) == "config.json")
        .min_by_key(|f| (!configs.contains(*f), file_name(f) != "config.json", depth(f), (*f).clone()));

    let named_dir = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| shallowest(dirs.iter().filter(|d| file_name(d).eq_ignore_ascii_case(name))))
    };

    let rules_dir = named_dir(&RULE_DIR_NAMES).map(|d| join(base, d)).or_else(|| {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for file in files.iter().filter(|f| extension(f).as_deref() == Some("srs")) {
            *counts.entry(parent(file)).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| depth(b.0).cmp(&depth(a.0))))
            .map(|(dir, _)| join(base, dir))
    });

    let bin_dir = shallowest(dirs.iter().filter(|d| file_name(d) == "bin"));
    let core_install_path = match (core, bin_dir) {
        (Some(core), _) => join(base, core),
        (None, Some(bin)) => join(base, &format!("{bin}/sing-box")),
        (None, None) => join(base, "sing-box"),
    };

    let singbox_path = core.map(|f| join(base, f));
    let config_path = config.map(|f| join(base, f));
    DetectedRuntimeFiles {
        base_dir: base.to_string(),
        found: singbox_path.is_some() && config_path.is_some(),
        singbox_path,
        core_install_path,
        config_path,
        rules_dir,
    }
}

fn split_listing(output: &str) -> (Vec<String>, Vec<String>) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("d ") {
            dirs.push(path.to_string());
        } else if let Some(path) = line.strip_prefix("f ") {
            files.push(path.to_string());
        }
    }
    (dirs, files)
}

fn quoted_list<'a>(paths: impl Iterator<Item = &'a String>) -> String {
    paths.map(|p| quote(p)).collect::<Vec<_>>().join(" ")
}

#[tauri::command]
pub async fn detect_runtime_files(base_dir: Option<String>) -> Result<DetectedRuntimeFiles, String> {
    let base = base_dir
        .map(|dir| dir.trim().trim_end_matches('/').to_string())
        .filter(|dir| !dir.is_empty())
        .ok_or("请先填写 sing-box 工作目录")?;
    if !base.starts_with('/') {
        return Err("工作目录需要是绝对路径".into());
    }
    if !root::enabled() {
        return Err("识别工作目录需要启用 root 模式".into());
    }

    let listing = root::run_async(format!(
        "cd {dir} || {{ echo '工作目录不存在: '{dir}; exit 1; }}\nfind . -maxdepth {SCAN_DEPTH} -mindepth 1 -type d 2>/dev/null | sed 's/^/d /'\nfind . -maxdepth {SCAN_DEPTH} -type f 2>/dev/null | sed 's/^/f /'",
        dir = quote(&base)
    ))
    .await?;
    let (dirs, files) = split_listing(&listing);

    let cores: Vec<&String> = files.iter().filter(|f| core_candidate(f)).collect();
    let jsons: Vec<&String> = files
        .iter()
        .filter(|f| config_candidate(f))
        .take(MAX_CONFIG_CANDIDATES)
        .collect();

    let mut elf = HashSet::new();
    let mut configs = HashSet::new();
    if !cores.is_empty() || !jsons.is_empty() {
        let checks = root::run_async(format!(
            "cd {dir} || exit 1\nfor f in {cores}; do [ \"$(head -c 4 \"$f\" 2>/dev/null | tail -c 3)\" = ELF ] && echo \"elf $f\"; done\nfor f in {jsons}; do grep -q '\"outbounds\"' \"$f\" 2>/dev/null && echo \"cfg $f\"; done\nexit 0",
            dir = quote(&base),
            cores = quoted_list(cores.iter().copied()),
            jsons = quoted_list(jsons.iter().copied()),
        ))
        .await?;
        for line in checks.lines() {
            if let Some(path) = line.strip_prefix("elf ") {
                elf.insert(path.to_string());
            } else if let Some(path) = line.strip_prefix("cfg ") {
                configs.insert(path.to_string());
            }
        }
    }

    Ok(pick(&base, &dirs, &files, &elf, &configs))
}

pub(crate) async fn run_singbox_check(
    singbox_path: &str,
    config_path: &str,
    working_dir: Option<&str>,
) -> Result<String, String> {
    if singbox_path.trim().is_empty() {
        return Err("未在工作目录中找到 sing-box 核心".into());
    }
    if config_path.trim().is_empty() {
        return Err("未在工作目录中找到配置文件".into());
    }
    if !root::enabled() {
        return Err("校验配置需要启用 root 模式".into());
    }
    let cwd = working_dir
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| parent(config_path).to_string());
    root::run_async(format!(
        "[ -f {core} ] || {{ echo 'sing-box 核心不存在: '{core}; exit 1; }}\n[ -f {config} ] || {{ echo '配置文件不存在: '{config}; exit 1; }}\ncd {cwd} || exit 1\ntimeout 60 {core} check -c {config} --disable-color",
        core = quote(singbox_path),
        cwd = quote(&cwd),
        config = quote(config_path)
    ))
    .await
    .map(|_| "Configuration is valid".to_string())
}

#[tauri::command]
pub async fn validate_config(
    singbox_path: String,
    config_path: String,
    working_dir: Option<String>,
) -> Result<String, String> {
    run_singbox_check(&singbox_path, &config_path, working_dir.as_deref()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn prefers_binaries_in_bin_and_configs_with_outbounds() {
        let dirs = strings(&["./bin", "./logs", "./rules", "./ui"]);
        let files = strings(&[
            "./sing-box.old",
            "./bin/sing-box",
            "./bin/yq",
            "./config.json",
            "./confs/work.json",
            "./ui/package.json",
            "./rules/geosite-cn.srs",
        ]);
        let elf = set(&["./sing-box.old", "./bin/sing-box", "./bin/yq"]);
        let configs = set(&["./confs/work.json"]);
        let detected = pick("/data/adb/box", &dirs, &files, &elf, &configs);
        assert_eq!(detected.singbox_path.as_deref(), Some("/data/adb/box/bin/sing-box"));
        assert_eq!(detected.config_path.as_deref(), Some("/data/adb/box/confs/work.json"));
        assert_eq!(detected.rules_dir.as_deref(), Some("/data/adb/box/rules"));
        assert_eq!(detected.core_install_path, "/data/adb/box/bin/sing-box");
        assert!(detected.found);
    }

    #[test]
    fn falls_back_to_working_directory_when_folders_are_missing() {
        let dirs = strings(&["./data"]);
        let files = strings(&["./data/cn.srs", "./data/ads.srs", "./config.json", "./singbox-arm64"]);
        let elf = set(&["./singbox-arm64"]);
        let detected = pick("/data/adb/sb", &dirs, &files, &elf, &HashSet::new());
        assert_eq!(detected.singbox_path.as_deref(), Some("/data/adb/sb/singbox-arm64"));
        assert_eq!(detected.config_path.as_deref(), Some("/data/adb/sb/config.json"));
        assert_eq!(detected.rules_dir.as_deref(), Some("/data/adb/sb/data"));
    }

    #[test]
    fn suggests_an_install_path_when_no_core_exists() {
        let with_bin = pick("/w", &strings(&["./bin"]), &[], &HashSet::new(), &HashSet::new());
        assert_eq!(with_bin.singbox_path, None);
        assert_eq!(with_bin.core_install_path, "/w/bin/sing-box");
        assert!(!with_bin.found);
        let bare = pick("/w", &[], &[], &HashSet::new(), &HashSet::new());
        assert_eq!(bare.core_install_path, "/w/sing-box");
        assert_eq!(bare.rules_dir, None);
    }

    #[test]
    fn only_binary_looking_files_are_core_candidates() {
        assert!(core_candidate("./bin/anything"));
        assert!(core_candidate("./sing-box"));
        assert!(!core_candidate("./sing-box.json"));
        assert!(!core_candidate("./bin/start.sh"));
        assert!(!core_candidate("./tools/clash"));
    }
}
