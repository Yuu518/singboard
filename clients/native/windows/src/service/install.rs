use super::{elevation, helper, runtime, scm};

pub fn configure(
    service: &str,
    snapshot: &runtime::Snapshot,
    user_sid: &str,
    start: bool,
) -> Result<(), String> {
    let status = scm::query_service_status(service)?;
    let installed = status.state != "not_installed";
    let previous = installed
        .then(|| runtime::Snapshot::read(service).ok())
        .flatten();
    let previous_image = helper::service_image_path(service);
    let was_running = matches!(status.state.as_str(), "running" | "starting");
    if was_running {
        scm::stop_service(service)?;
    }
    let mut created = false;
    let result = (|| {
        let deployed = helper::deploy_helper()?;
        let image = elevation::quote_arg(&deployed.to_string_lossy());
        if installed {
            scm::update_service_bin_path(service, &image)?;
        } else {
            scm::install_service(service, &image, service)?;
            created = true;
        }
        snapshot.publish(service)?;
        singboard_service::params::write_panel_sid(service, user_sid)?;
        if start || was_running {
            scm::start_service(service)?;
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        let mut errors = vec![error];
        if let Some(previous) = &previous {
            let rollback = (|| {
                scm::stop_service(service)?;
                previous.publish(service)?;
                if let Some(image) = &previous_image {
                    scm::update_service_bin_path(service, image)?;
                }
                if was_running {
                    scm::start_service(service)?;
                }
                Ok::<_, String>(())
            })();
            if let Err(error) = rollback {
                errors.push(format!("恢复旧服务失败: {error}"));
            } else {
                snapshot.discard_replaced_by(previous);
            }
        } else if created {
            if let Err(error) = scm::uninstall_service(service) {
                errors.push(format!("撤销服务安装失败: {error}"));
            }
        }
        return Err(errors.join("; "));
    }
    if let Some(previous) = previous {
        previous.discard_replaced_by(snapshot);
    }
    Ok(())
}
