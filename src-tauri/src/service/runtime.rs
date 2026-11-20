pub use super::snapshot::{Snapshot, capture, unique_name};
use super::snapshot::{capture_update, normalized_source_path, require_regular_source};
use super::{protected, scm};
use std::path::Path;

pub fn apply_update(
    user_sid: &str,
    service: &str,
    target: &Path,
    files: &[(String, Vec<u8>)],
    progress: &impl Fn(&str),
) -> Result<bool, String> {
    let status = scm::query_service_status(service)?;
    if status.state == "not_installed" {
        return crate::commands::update::platform::swap_and_restart(
            progress, files, target, service,
        );
    }
    let current = Snapshot::read(service)?;
    let _source_directory =
        protected::Directory::pin(current.source_core.parent().ok_or("源核心目录无效")?)?;
    let _target_directory = protected::Directory::pin(target.parent().ok_or("核心目标目录无效")?)?;
    require_regular_source(&current.source_core)?;
    require_regular_source(target)?;
    let source = normalized_source_path(&current.source_core)?;
    let resolved_target = normalized_source_path(target)?;
    if !source
        .to_string_lossy()
        .eq_ignore_ascii_case(&resolved_target.to_string_lossy())
    {
        return Err("所选核心路径与已安装服务的源路径不一致，请先重新安装服务".into());
    }
    let target = target.to_path_buf();
    let previous = if current.is_protected() {
        current.clone()
    } else {
        capture(
            &current.source_core,
            &current.source_config,
            &current.working_dir,
        )?
    };
    let next = capture_update(&previous, files)?;
    let was_running = matches!(status.state.as_str(), "running" | "starting");
    progress("replace");
    if was_running {
        scm::stop_service(service)?;
    }
    if let Err(error) = super::managed::configure(service, &previous, user_sid, false) {
        if was_running {
            let _ = scm::start_service(service);
        }
        next.discard_replaced_by(&previous);
        return Err(error);
    }
    let mut source_files = None;
    let result = (|| {
        source_files = Some(super::source_files::SourceFiles::replace(&target, files)?);
        next.publish(service)?;
        if was_running {
            progress("restart");
            scm::start_service(service)?;
            std::thread::sleep(std::time::Duration::from_secs(2));
            if scm::query_service_status(service)?.state != "running" {
                return Err("更新后的核心未保持运行".into());
            }
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        let mut errors = vec![error];
        if let Err(error) = scm::stop_service(service) {
            if let Some(source_files) = source_files.take() {
                source_files.commit();
            }
            errors.push(format!("停止更新后的服务失败: {error}"));
            return Err(errors.join("; "));
        }
        if let Some(source_files) = source_files.take() {
            if let Err(error) = source_files.rollback() {
                errors.push(format!("恢复源核心文件失败: {error}"));
            }
        }
        let rollback = previous.publish(service).and_then(|_| {
            if was_running {
                scm::start_service(service)
            } else {
                Ok(())
            }
        });
        if let Err(error) = rollback {
            errors.push(format!("恢复服务运行副本失败: {error}"));
        } else {
            next.discard_replaced_by(&previous);
        }
        return Err(errors.join("; "));
    }
    if let Some(source_files) = source_files {
        source_files.commit();
    }
    previous.discard_replaced_by(&next);
    Ok(was_running)
}
