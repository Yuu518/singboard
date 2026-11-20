use super::{protected, scm};
use singboard_service::params::{read_service_sources, write_service_sources};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub fn unique_name(prefix: &str) -> String {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{prefix}-{}-{time}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Clone)]
pub struct Snapshot {
    pub core: PathBuf,
    pub config: PathBuf,
    pub working_dir: String,
    pub source_core: PathBuf,
    pub source_config: PathBuf,
}

impl Snapshot {
    pub fn read(service: &str) -> Result<Self, String> {
        let (core, config, working_dir) = scm::read_service_params(service)?;
        let (source_core, source_config) = read_service_sources(service)?;
        Ok(Self {
            core: core.into(),
            config: config.into(),
            working_dir,
            source_core: source_core.into(),
            source_config: source_config.into(),
        })
    }

    pub fn publish(&self, service: &str) -> Result<(), String> {
        scm::write_service_params(
            service,
            &self.core.to_string_lossy(),
            &self.config.to_string_lossy(),
            &self.working_dir,
        )?;
        write_service_sources(
            service,
            &self.source_core.to_string_lossy(),
            &self.source_config.to_string_lossy(),
        )
    }

    pub fn is_protected(&self) -> bool {
        let Ok(root) = protected::Directory::open(false) else {
            return false;
        };
        let Some(parent) = self.core.parent() else {
            return false;
        };
        if !is_runtime_path(parent, root.path())
            || self.config.parent() != Some(parent)
            || self.core.file_name().and_then(|name| name.to_str()) != Some("sing-box.exe")
        {
            return false;
        }
        let Ok(directory) = protected::Directory::pin(parent) else {
            return false;
        };
        directory.validate().is_ok()
            && directory.validate_file(&self.core).is_ok()
            && directory.validate_file(&self.config).is_ok()
            && std::fs::read_dir(parent).is_ok_and(|entries| {
                entries.filter_map(Result::ok).all(|entry| {
                    !entry
                        .file_name()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".dll")
                        || directory.validate_file(&entry.path()).is_ok()
                })
            })
    }

    pub fn refresh_config(&self) -> Result<Self, String> {
        capture(&self.source_core, &self.source_config, &self.working_dir)
    }

    pub fn discard_replaced_by(&self, replacement: &Self) {
        if !self.is_protected() {
            return;
        }
        if self.core.parent() == replacement.core.parent() {
            if self.config != replacement.config {
                let _ = std::fs::remove_file(&self.config);
            }
            return;
        }
        if let Some(parent) = self.core.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

fn is_runtime_path(path: &Path, root: &Path) -> bool {
    path.parent() == Some(root)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.strip_prefix("runtime-").is_some_and(|value| {
                    let parts: Vec<_> = value.split('-').collect();
                    parts.len() == 3
                        && parts.iter().all(|part| {
                            !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())
                        })
                })
            })
}

fn read_core_files(core: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let parent = core.parent().ok_or("核心目录无效")?;
    let mut files = vec![(
        "sing-box.exe".into(),
        std::fs::read(core).map_err(|e| format!("读取核心失败: {e}"))?,
    )];
    for entry in std::fs::read_dir(parent).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_ascii_lowercase().ends_with(".dll") && entry.path().is_file() {
            files.push((
                name,
                std::fs::read(entry.path()).map_err(|e| e.to_string())?,
            ));
        }
    }
    Ok(files)
}

fn write_snapshot(
    files: &[(String, Vec<u8>)],
    config_bytes: &[u8],
    source_core: &Path,
    source_config: &Path,
    working_dir: &str,
) -> Result<Snapshot, String> {
    if !files.iter().any(|(name, _)| name == "sing-box.exe") {
        return Err("服务运行文件缺少 sing-box.exe".into());
    }
    let root = protected::Directory::open(true)?;
    let directory = root.create_child(&unique_name("runtime"))?;
    let result = (|| {
        for (name, bytes) in files {
            if name != "sing-box.exe" && !name.to_ascii_lowercase().ends_with(".dll") {
                return Err("服务运行文件名称无效".into());
            }
            directory.write_new(name, bytes)?;
        }
        let config = directory.write_new_config("config.json", config_bytes)?;
        Ok(Snapshot {
            core: directory.path().join("sing-box.exe"),
            config,
            working_dir: working_dir.into(),
            source_core: source_core.into(),
            source_config: source_config.into(),
        })
    })();
    let path = directory.path().to_path_buf();
    drop(directory);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(path);
    }
    result
}

pub fn capture(core: &Path, config: &Path, working_dir: &str) -> Result<Snapshot, String> {
    let _directory = protected::Directory::pin(core.parent().ok_or("源核心目录无效")?)?;
    require_regular_source(core)?;
    let files = read_core_files(core)?;
    let bytes = std::fs::read(config).map_err(|e| format!("读取源配置失败: {e}"))?;
    let working_dir = if working_dir.trim().is_empty() {
        config
            .parent()
            .ok_or("源配置目录无效")?
            .to_string_lossy()
            .to_string()
    } else {
        working_dir.to_string()
    };
    write_snapshot(&files, &bytes, core, config, &working_dir)
}

pub(super) fn require_regular_source(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Err("核心源文件不能是目录或重解析点".into());
    }
    Ok(())
}

pub(super) fn normalized_source_path(path: &Path) -> Result<PathBuf, String> {
    let parent = path.parent().ok_or("源核心目录无效")?;
    let name = path.file_name().ok_or("源核心文件名无效")?;
    Ok(std::fs::canonicalize(parent)
        .map_err(|e| e.to_string())?
        .join(name))
}

pub fn capture_update(current: &Snapshot, files: &[(String, Vec<u8>)]) -> Result<Snapshot, String> {
    let config =
        std::fs::read(&current.source_config).map_err(|e| format!("读取源配置失败: {e}"))?;
    write_snapshot(
        files,
        &config,
        &current.source_core,
        &current.source_config,
        &current.working_dir,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_paths_must_be_direct_children_of_the_protected_root() {
        let root = Path::new(r"C:\Program Files\singboard-service");
        assert!(is_runtime_path(&root.join("runtime-12-345-0"), root));
        assert!(!is_runtime_path(
            &root.join("runtime-12-345-0").join("child"),
            root
        ));
        assert!(!is_runtime_path(
            Path::new(r"C:\Users\Public\runtime-12-345-0"),
            root
        ));
        assert!(!is_runtime_path(&root.join("runtime-user-files"), root));
        assert!(!is_runtime_path(&root.join("runtime-"), root));
    }
}
