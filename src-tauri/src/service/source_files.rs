use super::protected::Directory;
use std::collections::HashSet;
use std::io::Write;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::System::Com::CoCreateGuid;

struct Replacement {
    destination: PathBuf,
    backup: Option<PathBuf>,
}

struct StagedFile(PathBuf);

impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub struct SourceFiles {
    directory: Directory,
    replacements: Vec<Replacement>,
    finished: bool,
}

impl SourceFiles {
    pub fn replace(target: &Path, files: &[(String, Vec<u8>)]) -> Result<Self, String> {
        let parent = target.parent().ok_or("源核心目录无效")?;
        let directory = Directory::pin(parent)?;
        let destinations = destinations(target, files)?;
        let mut transaction = Self {
            directory,
            replacements: Vec::new(),
            finished: false,
        };
        for (destination, (_, bytes)) in destinations.iter().zip(files) {
            if let Err(error) = transaction.replace_file(destination, bytes) {
                return match transaction.rollback() {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(format!("{error}; {rollback}")),
                };
            }
        }
        Ok(transaction)
    }

    fn replace_file(&mut self, destination: &Path, bytes: &[u8]) -> Result<(), String> {
        let had_old = match std::fs::symlink_metadata(destination) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                {
                    return Err(format!(
                        "源核心文件不能是目录或重解析点: {}",
                        destination.display()
                    ));
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(format!("读取源核心文件信息失败: {error}")),
        };
        let staged = StagedFile(self.directory.path().join(unique_name("new")?));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged.0)
            .map_err(|error| format!("暂存源核心文件失败: {error}"))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("写入源核心文件失败: {error}"))?;
        drop(file);
        let backup = if had_old {
            let backup = self.directory.path().join(unique_name("backup")?);
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&backup)
                .map_err(|error| format!("创建源核心备份失败: {error}"))?;
            if let Err(error) =
                crate::commands::update::retry_io(3, || std::fs::rename(destination, &backup))
            {
                let _ = std::fs::remove_file(&backup);
                return Err(format!("备份源核心文件失败: {error}"));
            }
            Some(backup)
        } else {
            None
        };
        self.replacements.push(Replacement {
            destination: destination.to_path_buf(),
            backup,
        });
        std::fs::rename(&staged.0, destination)
            .map_err(|error| format!("替换源核心文件失败: {error}"))?;
        Ok(())
    }

    pub fn commit(mut self) {
        self.finished = true;
        for replacement in &self.replacements {
            if let Some(backup) = &replacement.backup {
                let _ = std::fs::remove_file(backup);
            }
        }
    }

    pub fn rollback(mut self) -> Result<(), String> {
        self.restore()
    }

    fn restore(&mut self) -> Result<(), String> {
        self.finished = true;
        let mut errors = Vec::new();
        for replacement in self.replacements.iter().rev() {
            let result = (|| {
                match std::fs::remove_file(&replacement.destination) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                if let Some(backup) = &replacement.backup {
                    std::fs::rename(backup, &replacement.destination)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(format!(
                    "恢复源核心文件 {} 失败: {error}",
                    replacement.destination.display()
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

impl Drop for SourceFiles {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.restore();
        }
    }
}

fn destinations(target: &Path, files: &[(String, Vec<u8>)]) -> Result<Vec<PathBuf>, String> {
    let parent = target.parent().ok_or("源核心目录无效")?;
    let mut names = HashSet::new();
    let mut core_seen = false;
    let mut destinations = Vec::new();
    for (name, _) in files {
        if name.is_empty()
            || name.ends_with(['.', ' '])
            || name.chars().any(|c| {
                c.is_control() || matches!(c, '/' | '\\' | ':' | '"' | '<' | '>' | '|' | '?' | '*')
            })
        {
            return Err("源核心文件名称无效".into());
        }
        let destination = if name == "sing-box.exe" {
            core_seen = true;
            target.to_path_buf()
        } else if name.to_ascii_lowercase().ends_with(".dll") {
            parent.join(name)
        } else {
            return Err("源核心文件名称无效".into());
        };
        if !names.insert(destination.to_string_lossy().to_lowercase()) {
            return Err("源核心更新包含重复文件".into());
        }
        destinations.push(destination);
    }
    if !core_seen {
        return Err("源核心更新缺少 sing-box.exe".into());
    }
    Ok(destinations)
}

fn unique_name(kind: &str) -> Result<String, String> {
    let mut guid = unsafe { std::mem::zeroed() };
    let status = unsafe { CoCreateGuid(&mut guid) };
    if status < 0 {
        return Err(format!("创建源核心临时文件名失败: {status}"));
    }
    Ok(format!(
        ".singboard-{kind}-{:08x}{:04x}{:04x}{:016x}",
        guid.data1,
        guid.data2,
        guid.data3,
        u64::from_be_bytes(guid.data4)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(unique_name("source-test").unwrap());
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn assert_no_temporary_files(&self) {
            assert!(std::fs::read_dir(&self.0).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".singboard-")
            }));
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn new_files() -> Vec<(String, Vec<u8>)> {
        vec![
            ("sing-box.exe".into(), b"new core".to_vec()),
            ("libcronet.dll".into(), b"new dll".to_vec()),
        ]
    }

    #[test]
    fn commits_the_core_and_dll_together_without_leaving_backups() {
        let directory = TestDirectory::new();
        let target = directory.0.join("custom-core.exe");
        let dll = directory.0.join("libcronet.dll");
        std::fs::write(&target, b"old core").unwrap();
        std::fs::write(&dll, b"old dll").unwrap();

        SourceFiles::replace(&target, &new_files())
            .unwrap()
            .commit();

        assert_eq!(std::fs::read(target).unwrap(), b"new core");
        assert_eq!(std::fs::read(dll).unwrap(), b"new dll");
        directory.assert_no_temporary_files();
    }

    #[test]
    fn failure_on_a_locked_dll_restores_the_already_replaced_core() {
        let directory = TestDirectory::new();
        let target = directory.0.join("sing-box.exe");
        let dll = directory.0.join("libcronet.dll");
        std::fs::write(&target, b"old core").unwrap();
        std::fs::write(&dll, b"old dll").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&dll)
            .unwrap();

        assert!(SourceFiles::replace(&target, &new_files()).is_err());

        assert_eq!(std::fs::read(target).unwrap(), b"old core");
        assert_eq!(std::fs::read(&dll).unwrap(), b"old dll");
        directory.assert_no_temporary_files();
        drop(locked);
    }

    #[test]
    fn explicit_rollback_restores_original_files() {
        let directory = TestDirectory::new();
        let target = directory.0.join("sing-box.exe");
        let dll = directory.0.join("libcronet.dll");
        std::fs::write(&target, b"old core").unwrap();
        std::fs::write(&dll, b"old dll").unwrap();

        SourceFiles::replace(&target, &new_files())
            .unwrap()
            .rollback()
            .unwrap();

        assert_eq!(std::fs::read(target).unwrap(), b"old core");
        assert_eq!(std::fs::read(dll).unwrap(), b"old dll");
        directory.assert_no_temporary_files();
    }

    #[test]
    fn dropping_an_uncommitted_transaction_restores_original_files() {
        let directory = TestDirectory::new();
        let target = directory.0.join("sing-box.exe");
        std::fs::write(&target, b"old core").unwrap();

        drop(SourceFiles::replace(&target, &new_files()).unwrap());

        assert_eq!(std::fs::read(target).unwrap(), b"old core");
        assert!(!directory.0.join("libcronet.dll").exists());
        directory.assert_no_temporary_files();
    }

    #[test]
    fn missing_original_files_can_be_installed_and_rolled_back() {
        let directory = TestDirectory::new();
        let target = directory.0.join("sing-box.exe");
        let dll = directory.0.join("libcronet.dll");

        let transaction = SourceFiles::replace(&target, &new_files()).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new core");
        assert_eq!(std::fs::read(&dll).unwrap(), b"new dll");
        transaction.rollback().unwrap();

        assert!(!target.exists());
        assert!(!dll.exists());
        directory.assert_no_temporary_files();
    }

    #[test]
    fn rollback_continues_restoring_other_files_after_a_locked_destination() {
        let directory = TestDirectory::new();
        let target = directory.0.join("sing-box.exe");
        let dll = directory.0.join("libcronet.dll");
        std::fs::write(&target, b"old core").unwrap();
        std::fs::write(&dll, b"old dll").unwrap();
        let transaction = SourceFiles::replace(&target, &new_files()).unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&dll)
            .unwrap();

        assert!(transaction.rollback().is_err());

        assert_eq!(std::fs::read(target).unwrap(), b"old core");
        assert_eq!(std::fs::read(&dll).unwrap(), b"new dll");
        let backups: Vec<_> = std::fs::read_dir(&directory.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".singboard-backup-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), b"old dll");
        drop(locked);
    }

    #[test]
    fn invalid_or_duplicate_destinations_are_rejected_before_replacement() {
        let directory = TestDirectory::new();
        let target = directory.0.join("sing-box.exe");
        std::fs::write(&target, b"old core").unwrap();
        for name in [
            "../outside.dll",
            "C:outside.dll",
            "nested\\outside.dll",
            "bad\n.dll",
        ] {
            let files = vec![
                ("sing-box.exe".into(), b"new core".to_vec()),
                (name.into(), b"new dll".to_vec()),
            ];
            assert!(SourceFiles::replace(&target, &files).is_err());
        }
        let mut files = new_files();
        files.push(("LIBCRONET.DLL".into(), b"another dll".to_vec()));
        assert!(SourceFiles::replace(&target, &files).is_err());

        assert_eq!(std::fs::read(target).unwrap(), b"old core");
        directory.assert_no_temporary_files();
    }
}
