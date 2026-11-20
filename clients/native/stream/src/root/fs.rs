use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{enabled, exec, quote, run};

fn root_only(path: &Path) -> bool {
    path.starts_with("/data/adb")
}

fn wants_root(path: &Path, error: Option<&std::io::Error>) -> bool {
    if !enabled() {
        return false;
    }
    if root_only(path) {
        return true;
    }
    matches!(error.map(std::io::Error::kind), Some(ErrorKind::PermissionDenied))
}

fn path_arg(path: &Path) -> String {
    quote(&path.to_string_lossy())
}

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    if enabled() && root_only(path) {
        return read_root(path);
    }
    match std::fs::read(path) {
        Ok(data) => Ok(data),
        Err(e) if wants_root(path, Some(&e)) => read_root(path),
        Err(e) => Err(format!("读取 {} 失败: {e}", path.display())),
    }
}

fn read_root(path: &Path) -> Result<Vec<u8>, String> {
    let p = path_arg(path);
    let (code, output) = exec(&format!("[ -f {p} ] || exit 2\nbase64 {p} 2>/dev/null"))?;
    match code {
        0 => {
            let cleaned: Vec<u8> = output
                .into_iter()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            STANDARD
                .decode(cleaned)
                .map_err(|e| format!("读取 {} 失败: {e}", path.display()))
        }
        2 => Err(format!("文件不存在: {}", path.display())),
        _ => Err(format!("读取 {} 失败", path.display())),
    }
}

pub fn read_to_string(path: &Path) -> Result<String, String> {
    let data = read(path)?;
    String::from_utf8(data).map_err(|_| format!("{} 不是有效的 UTF-8 文本", path.display()))
}

pub fn write(path: &Path, data: &[u8]) -> Result<(), String> {
    forget_shared(path);
    if enabled() && root_only(path) {
        return write_root(path, data);
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            if wants_root(path, Some(&e)) {
                return write_root(path, data);
            }
            return Err(format!("创建目录 {} 失败: {e}", parent.display()));
        }
    }
    match std::fs::write(path, data) {
        Ok(()) => Ok(()),
        Err(e) if wants_root(path, Some(&e)) => write_root(path, data),
        Err(e) => Err(format!("写入 {} 失败: {e}", path.display())),
    }
}

fn write_root(path: &Path, data: &[u8]) -> Result<(), String> {
    let encoded = STANDARD.encode(data);
    let mut body = String::with_capacity(encoded.len() + encoded.len() / 76 + 1);
    for chunk in encoded.as_bytes().chunks(76) {
        body.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        body.push('\n');
    }
    let p = path_arg(path);
    let tmp = quote(&format!("{}.singboard-tmp", path.to_string_lossy()));
    let parent = path
        .parent()
        .map(|d| quote(&d.to_string_lossy()))
        .unwrap_or_else(|| "/".into());
    run(&format!(
        "set -e\nmkdir -p {parent}\nbase64 -d > {tmp} <<'__SINGBOARD_DATA__'\n{body}__SINGBOARD_DATA__\nchmod 600 {tmp}\nmv -f {tmp} {p}"
    ))
    .map(|_| ())
    .map_err(|e| format!("写入 {} 失败: {e}", path.display()))
}

pub fn remove(path: &Path) -> Result<(), String> {
    forget_shared(path);
    if enabled() && root_only(path) {
        return run(&format!("rm -f {}", path_arg(path))).map(|_| ());
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) if wants_root(path, Some(&e)) => {
            run(&format!("rm -f {}", path_arg(path))).map(|_| ())
        }
        Err(e) => Err(format!("删除 {} 失败: {e}", path.display())),
    }
}

fn test_root(flag: &str, path: &Path) -> bool {
    matches!(exec(&format!("[ {flag} {} ]", path_arg(path))), Ok((0, _)))
}

pub fn is_file(path: &Path) -> bool {
    if enabled() && root_only(path) {
        return test_root("-f", path);
    }
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file(),
        Err(e) if wants_root(path, Some(&e)) => test_root("-f", path),
        Err(_) => false,
    }
}

pub fn is_dir(path: &Path) -> bool {
    if enabled() && root_only(path) {
        return test_root("-d", path);
    }
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_dir(),
        Err(e) if wants_root(path, Some(&e)) => test_root("-d", path),
        Err(_) => false,
    }
}

pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
}

pub fn list_dir(dir: &Path) -> Result<Vec<Entry>, String> {
    if enabled() && root_only(dir) {
        return list_dir_root(dir);
    }
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(entries
            .flatten()
            .map(|entry| {
                let path = entry.path();
                let is_dir = path.is_dir();
                Entry { path, is_dir }
            })
            .collect()),
        Err(e) if wants_root(dir, Some(&e)) => list_dir_root(dir),
        Err(e) => Err(format!("读取目录 {} 失败: {e}", dir.display())),
    }
}

fn list_dir_root(dir: &Path) -> Result<Vec<Entry>, String> {
    let output = run(&format!(
        "cd {} || exit 1\nfor f in * .[!.]*; do\n  [ -e \"$f\" ] || continue\n  if [ -d \"$f\" ]; then echo \"d $f\"; else echo \"f $f\"; fi\ndone",
        path_arg(dir)
    ))?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let (kind, name) = line.split_once(' ')?;
            Some(Entry {
                path: dir.join(name),
                is_dir: kind == "d",
            })
        })
        .collect())
}

pub fn sha256(path: &Path) -> Result<String, String> {
    if enabled() && root_only(path) {
        let output = run(&format!("sha256sum {}", path_arg(path)))?;
        return output
            .split_whitespace()
            .next()
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| format!("计算 {} 的哈希失败", path.display()));
    }
    let data = read(path)?;
    Ok(format!("{:x}", Sha256::digest(&data)))
}

const SHARED_TTL: Duration = Duration::from_secs(5);

static SHARED: Mutex<Option<(PathBuf, Instant, Arc<Vec<u8>>)>> = Mutex::new(None);

fn forget_shared(path: &Path) {
    if let Ok(mut guard) = SHARED.lock() {
        if guard.as_ref().is_some_and(|(cached, _, _)| cached == path) {
            *guard = None;
        }
    }
}

pub fn read_shared(path: &Path) -> Result<Arc<Vec<u8>>, String> {
    let mut guard = SHARED.lock().map_err(|_| "文件缓存状态异常".to_string())?;
    if let Some((cached, at, data)) = guard.as_ref() {
        if cached == path && at.elapsed() < SHARED_TTL {
            return Ok(data.clone());
        }
    }
    let data = Arc::new(read(path)?);
    *guard = Some((path.to_path_buf(), Instant::now(), data.clone()));
    Ok(data)
}
