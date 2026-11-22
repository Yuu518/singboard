use super::*;
use gpui_component::input::InputState;
use singboard_core::{
    config_files,
    profiles::{download, validate_jsonc},
    settings::{ConfigProfile, Settings},
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

fn new_profile_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("p-{nanos:x}-{seq:x}")
}

pub(super) enum ConfigDialog {
    AddLocal {
        name: Entity<InputState>,
        path: Entity<InputState>,
        error: String,
        busy: bool,
    },
    AddRemote {
        name: Entity<InputState>,
        url: Entity<InputState>,
        interval: Entity<InputState>,
        error: String,
        busy: bool,
    },
    EditInfo {
        id: String,
        is_remote: bool,
        name: Entity<InputState>,
        source: Entity<InputState>,
        interval: Option<Entity<InputState>>,
        error: String,
        busy: bool,
    },
    ConfirmDelete {
        id: String,
        name: String,
        is_remote: bool,
        is_active: bool,
    },
}

fn remote_config_path(id: &str) -> Result<PathBuf, String> {
    Ok(crate::service::data_dir()?
        .join("remote-configs")
        .join(format!("{id}.json")))
}

fn profile_file_path(profile: &ConfigProfile) -> Result<PathBuf, String> {
    if profile.kind == "remote" {
        remote_config_path(&profile.id)
    } else {
        Ok(PathBuf::from(&profile.source))
    }
}

fn has_backup(profile: &ConfigProfile) -> bool {
    profile_file_path(profile)
        .map(|p| config_files::config_backup_path(&p).exists())
        .unwrap_or(false)
}

fn source_label(kind: &str, source: &str) -> String {
    if kind == "remote" {
        if let Some(rest) = source
            .strip_prefix("https://")
            .or_else(|| source.strip_prefix("http://"))
        {
            return rest.split('/').next().unwrap_or(source).to_owned();
        }
    }
    source.to_owned()
}

fn short_timestamp(iso: &str) -> String {
    if iso.is_empty() {
        return String::new();
    }
    let clean = iso.trim_end_matches('Z');
    let mut parts = clean.split('T');
    let date = parts.next().unwrap_or("");
    let time = parts.next().unwrap_or("");
    let md = date.get(5..10).unwrap_or(date);
    let hm = time.get(0..5).unwrap_or(time);
    if !md.is_empty() && !hm.is_empty() {
        format!("{md} {hm}")
    } else {
        date.to_owned()
    }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn parse_iso_seconds(iso: &str) -> Option<i64> {
    let clean = iso.trim().trim_end_matches('Z');
    let (date, time) = clean.split_once('T')?;
    let mut dp = date.split('-');
    let y: i64 = dp.next()?.parse().ok()?;
    let m: i64 = dp.next()?.parse().ok()?;
    let d: i64 = dp.next()?.parse().ok()?;
    let mut tp = time.split(':');
    let hh: i64 = tp.next()?.parse().ok()?;
    let mm: i64 = tp.next()?.parse().ok()?;
    let ss: i64 = tp.next()?.split('.').next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss)
}

fn is_remote_profile_due(profile: &ConfigProfile, now_secs: i64) -> bool {
    if profile.kind != "remote" || profile.auto_update_interval == 0 {
        return false;
    }
    let Some(last_secs) = parse_iso_seconds(&profile.last_updated) else {
        return true;
    };
    now_secs.saturating_sub(last_secs) >= i64::from(profile.auto_update_interval) * 3600
}

async fn sync_service_config(path: &Path) -> Result<(), String> {
    let status = tokio::task::spawn_blocking(crate::service::status)
        .await
        .map_err(|e| e.to_string())??;
    if status.state == "not_installed" {
        return Ok(());
    }
    if let Ok(Some(snapshot)) = tokio::task::spawn_blocking(crate::service::snapshot)
        .await
        .map_err(|e| e.to_string())?
    {
        if snapshot
            .source_config
            .to_string_lossy()
            .eq_ignore_ascii_case(&path.to_string_lossy())
        {
            return Ok(());
        }
    }
    crate::service::request(crate::service::admin::Operation::UseConfig {
        service: status.name,
        config: path.to_path_buf(),
    })
    .await
}

fn resolve_local_input(raw: &str, working_dir: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("请填写文件路径".into());
    }
    let path = Path::new(trimmed);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else if !working_dir.trim().is_empty() {
        Path::new(working_dir.trim()).join(path)
    } else {
        return Err("未配置工作目录，请填写配置文件的绝对路径".into());
    };
    if !full.is_file() {
        return Err(format!("找不到文件：{}", full.display()));
    }
    Ok(full)
}

impl Workspace {
    fn open_add_local_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("例如：默认配置"));
        let path = cx.new(|cx| {
            InputState::new(window, cx).placeholder("config.json 或 C:\\sing-box\\config.json")
        });
        self.config_dialog = Some(ConfigDialog::AddLocal {
            name,
            path,
            error: String::new(),
            busy: false,
        });
        cx.notify();
    }

    fn open_add_remote_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("例如：远程订阅"));
        let url =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://example.com/config.json"));
        let interval = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        self.config_dialog = Some(ConfigDialog::AddRemote {
            name,
            url,
            interval,
            error: String::new(),
            busy: false,
        });
        cx.notify();
    }

    fn open_edit_info_dialog(
        &mut self,
        profile: &ConfigProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_remote = profile.kind == "remote";
        let name = cx.new(|cx| InputState::new(window, cx).default_value(profile.name.clone()));
        let source = cx.new(|cx| InputState::new(window, cx).default_value(profile.source.clone()));
        let interval = is_remote.then(|| {
            cx.new(|cx| {
                InputState::new(window, cx).default_value(profile.auto_update_interval.to_string())
            })
        });
        self.config_dialog = Some(ConfigDialog::EditInfo {
            id: profile.id.clone(),
            is_remote,
            name,
            source,
            interval,
            error: String::new(),
            busy: false,
        });
        cx.notify();
    }

    fn browse_dialog_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择 sing-box 配置文件".into()),
        });
        let handle = window.window_handle();
        cx.spawn(async move |weak, cx| {
            if let Ok(Ok(Some(paths))) = picker.await
                && let Some(file) = paths.into_iter().next()
            {
                let value = file.to_string_lossy().into_owned();
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        match &mut this.config_dialog {
                            Some(ConfigDialog::AddLocal { path, error, .. }) => {
                                path.update(cx, |i, cx| i.set_value(value, window, cx));
                                error.clear();
                            }
                            Some(ConfigDialog::EditInfo {
                                source,
                                error,
                                is_remote: false,
                                ..
                            }) => {
                                source.update(cx, |i, cx| i.set_value(value, window, cx));
                                error.clear();
                            }
                            _ => {}
                        }
                        cx.notify();
                    });
                });
            }
        })
        .detach();
    }

    fn submit_add_local(&mut self, cx: &mut Context<Self>) {
        let (name_val, path_val) = match &mut self.config_dialog {
            Some(ConfigDialog::AddLocal {
                name,
                path,
                busy,
                error,
            }) => {
                if *busy {
                    return;
                }
                let n = name.read(cx).value().trim().to_owned();
                let p = path.read(cx).value().trim().to_owned();
                if n.is_empty() || p.is_empty() {
                    *error = "请填写名称和文件路径".into();
                    cx.notify();
                    return;
                }
                *busy = true;
                error.clear();
                (n, p)
            }
            _ => return,
        };
        let task = singboard_core::runtime().spawn_blocking(move || {
            let dir = crate::service::data_dir()?;
            let prefs = crate::maintenance::preferences()?;
            let resolved = resolve_local_input(&path_val, &prefs.working_dir)?;
            let text =
                std::fs::read_to_string(&resolved).map_err(|e| format!("无法读取配置文件：{e}"))?;
            validate_jsonc(&text)?;
            let resolved_str = resolved.to_string_lossy().into_owned();
            if let Some(dup) = prefs
                .config_profiles
                .iter()
                .find(|p| p.kind == "local" && p.source.eq_ignore_ascii_case(&resolved_str))
            {
                return Err(format!("这个文件已经添加为「{}」", dup.name));
            }
            let now = super::now_iso();
            Settings::edit(&dir, |p| {
                p.config_profiles.push(ConfigProfile {
                    id: new_profile_id(),
                    name: name_val.clone(),
                    source: resolved_str.clone(),
                    kind: "local".into(),
                    auto_update_interval: 0,
                    last_updated: now,
                    ..Default::default()
                });
                Ok(())
            })?;
            Ok::<_, String>(format!("已添加本地配置「{name_val}」，点「使用」即可启用"))
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                match result {
                    Ok(msg) => {
                        this.close_config_dialog(cx);
                        this.message(msg,false,cx);
                    }
                    Err(err) => {
                        if let Some(ConfigDialog::AddLocal { busy, error, .. }) =
                            &mut this.config_dialog
                        {
                            *busy = false;
                            *error = err;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn submit_add_remote(&mut self, cx: &mut Context<Self>) {
        let (name_val, url_val, hours) = match &mut self.config_dialog {
            Some(ConfigDialog::AddRemote {
                name,
                url,
                interval,
                busy,
                error,
            }) => {
                if *busy {
                    return;
                }
                let n = name.read(cx).value().trim().to_owned();
                let u = url.read(cx).value().trim().to_owned();
                if n.is_empty() || u.is_empty() {
                    *error = "请填写名称和远程地址".into();
                    cx.notify();
                    return;
                }
                if !u.starts_with("http://") && !u.starts_with("https://") {
                    *error = "请输入以 http:// 或 https:// 开头的远程地址".into();
                    cx.notify();
                    return;
                }
                let h = interval
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u32>()
                    .unwrap_or(0)
                    .min(576);
                *busy = true;
                error.clear();
                (n, u, h)
            }
            _ => return,
        };
        let task = singboard_core::runtime().spawn(async move {
            let dir = crate::service::data_dir()?;
            let prefs = crate::maintenance::preferences()?;
            if let Some(dup) = prefs
                .config_profiles
                .iter()
                .find(|p| p.kind == "remote" && p.source == url_val)
            {
                return Err(format!("这个订阅地址已存在：「{}」", dup.name));
            }
            let text = download(&url_val).await?;
            let id = new_profile_id();
            let dest = remote_config_path(&id)?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if !prefs.singbox_path.trim().is_empty() {
                let tmp = dest.with_extension("tmp.json");
                std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
                let check = config_files::run_singbox_check(
                    &prefs.singbox_path,
                    &tmp.to_string_lossy(),
                    (!prefs.working_dir.trim().is_empty()).then_some(prefs.working_dir.as_str()),
                )
                .await;
                let _ = std::fs::remove_file(&tmp);
                check?;
            }
            singboard_core::profiles::write_atomic(&dest, text.as_bytes())?;
            let now = super::now_iso();
            Settings::edit(&dir, |p| {
                p.config_profiles.push(ConfigProfile {
                    id: id.clone(),
                    name: name_val.clone(),
                    source: url_val.clone(),
                    kind: "remote".into(),
                    auto_update_interval: hours,
                    last_updated: now,
                    ..Default::default()
                });
                Ok(())
            })?;
            Ok::<_, String>(format!("已添加远程配置「{name_val}」，点「使用」即可启用"))
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                match result {
                    Ok(msg) => {
                        this.close_config_dialog(cx);
                        this.message(msg,false,cx);
                    }
                    Err(err) => {
                        if let Some(ConfigDialog::AddRemote { busy, error, .. }) =
                            &mut this.config_dialog
                        {
                            *busy = false;
                            *error = format!("下载或校验失败，没有添加：{err}");
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn submit_edit_info(&mut self, cx: &mut Context<Self>) {
        let (id, is_remote, name_val, source_val, hours) = match &mut self.config_dialog {
            Some(ConfigDialog::EditInfo {
                id,
                is_remote,
                name,
                source,
                interval,
                busy,
                error,
            }) => {
                if *busy {
                    return;
                }
                let n = name.read(cx).value().trim().to_owned();
                let s = source.read(cx).value().trim().to_owned();
                if n.is_empty() || s.is_empty() {
                    *error = if *is_remote {
                        "请填写名称和远程地址".into()
                    } else {
                        "请填写名称和文件路径".into()
                    };
                    cx.notify();
                    return;
                }
                let h = interval
                    .as_ref()
                    .and_then(|e| e.read(cx).value().trim().parse::<u32>().ok())
                    .unwrap_or(0)
                    .min(576);
                *busy = true;
                error.clear();
                (id.clone(), *is_remote, n, s, h)
            }
            _ => return,
        };
        let task = singboard_core::runtime().spawn(async move {
            let dir = crate::service::data_dir()?;
            let prefs = crate::maintenance::preferences()?;
            let before = prefs
                .config_profiles
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .ok_or("配置已不存在")?;
            let is_active = prefs.active_config_profile_id == id;
            let mut next_source = source_val.clone();
            let mut restarted_note: Option<String> = None;

            if is_remote {
                if source_val != before.source {
                    if !source_val.starts_with("http://") && !source_val.starts_with("https://") {
                        return Err("请输入以 http:// 或 https:// 开头的远程地址".into());
                    }
                    let text = download(&source_val).await?;
                    let dest = remote_config_path(&id)?;
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    if !prefs.singbox_path.trim().is_empty() {
                        let tmp = dest.with_extension("tmp.json");
                        std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
                        let check = config_files::run_singbox_check(
                            &prefs.singbox_path,
                            &tmp.to_string_lossy(),
                            (!prefs.working_dir.trim().is_empty())
                                .then_some(prefs.working_dir.as_str()),
                        )
                        .await;
                        let _ = std::fs::remove_file(&tmp);
                        check?;
                    }
                    config_files::replace_with_backup(&dest, text.as_bytes())?;
                    if is_active {
                        restarted_note = Some(format!(
                            "「{name_val}」改用了新的远程地址，重启核心后生效。"
                        ));
                    }
                }
            } else {
                let resolved = resolve_local_input(&source_val, &prefs.working_dir)?;
                next_source = resolved.to_string_lossy().into_owned();
                if !next_source.eq_ignore_ascii_case(&before.source) {
                    let text = std::fs::read_to_string(&resolved)
                        .map_err(|e| format!("无法读取配置文件：{e}"))?;
                    validate_jsonc(&text)?;
                    if is_active && !prefs.singbox_path.trim().is_empty() {
                        config_files::run_singbox_check(
                            &prefs.singbox_path,
                            &next_source,
                            (!prefs.working_dir.trim().is_empty())
                                .then_some(prefs.working_dir.as_str()),
                        )
                        .await?;
                        restarted_note =
                            Some(format!("「{name_val}」改用了新的源文件，重启核心后生效。"));
                    }
                }
            }

            let now = super::now_iso();
            if is_active && !is_remote && !next_source.eq_ignore_ascii_case(&before.source) {
                sync_service_config(Path::new(&next_source)).await?;
            }
            Settings::edit(&dir, |p| {
                if let Some(entry) = p.config_profiles.iter_mut().find(|v| v.id == id) {
                    entry.name = name_val.clone();
                    if is_remote && next_source != entry.source {
                        entry.last_updated = now.clone();
                        entry.last_fetch_error.clear();
                    }
                    entry.source = next_source.clone();
                    if is_remote {
                        entry.auto_update_interval = hours;
                    }
                }
                if is_active && !is_remote {
                    p.config_path = next_source;
                }
                Ok(())
            })?;
            Ok::<_, String>((format!("已保存「{name_val}」"), restarted_note))
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                match result {
                    Ok((msg, note)) => {
                        this.close_config_dialog(cx);
                        this.message(msg,false,cx);
                        if let Some(n) = note {
                            this.pending_restart = Some(n);
                        }
                    }
                    Err(err) => {
                        if let Some(ConfigDialog::EditInfo { busy, error, .. }) =
                            &mut this.config_dialog
                        {
                            *busy = false;
                            *error = err;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn select_profile(&mut self, profile: ConfigProfile, cx: &mut Context<Self>) {
        if self.config_busy.is_some() {
            return;
        }
        let id = profile.id.clone();
        let name = profile.name.clone();
        self.config_busy = Some(id.clone());
        self.load_error.clear();
        let task = singboard_core::runtime().spawn(async move {
            let dir = crate::service::data_dir()?;
            let prefs = crate::maintenance::preferences()?;
            let path = profile_file_path(&profile)?;
            if !path.is_file() {
                return Err(format!("找不到配置文件：{}", path.display()));
            }
            if !prefs.singbox_path.trim().is_empty() {
                config_files::run_singbox_check(
                    &prefs.singbox_path,
                    &path.to_string_lossy(),
                    (!prefs.working_dir.trim().is_empty()).then_some(prefs.working_dir.as_str()),
                )
                .await?;
            }
            sync_service_config(&path).await?;
            let path_str = path.to_string_lossy().into_owned();
            Settings::edit(&dir, |p| {
                p.active_config_profile_id = id.clone();
                p.config_path = path_str;
                Ok(())
            })?;
            Ok::<_, String>(name)
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                this.config_busy = None;
                match result {
                    Ok(name) => {
                        this.message(format!("已选用配置「{name}」"),false,cx);
                        this.pending_restart =
                            Some(format!("已切换到「{name}」，重启核心后生效。"));
                    }
                    Err(err) => {
                        this.message(format!("选用配置失败：{err}"),true,cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn poll_due_remote_profiles(&mut self, cx: &mut Context<Self>) {
        if self.auto_updating_profiles || self.config_busy.is_some() {
            return;
        }
        if self
            .last_profile_auto_check
            .is_some_and(|last| last.elapsed() < std::time::Duration::from_secs(300))
        {
            return;
        }
        self.last_profile_auto_check = Some(std::time::Instant::now());
        self.auto_updating_profiles = true;
        let task = singboard_core::runtime().spawn(async move {
            let dir = crate::service::data_dir()?;
            let prefs = crate::maintenance::preferences()?;
            let Some(now_secs) = parse_iso_seconds(&super::now_iso()) else {
                return Ok::<_, String>((Vec::new(), Vec::new(), false));
            };
            let due: Vec<ConfigProfile> = prefs
                .config_profiles
                .iter()
                .filter(|p| is_remote_profile_due(p, now_secs))
                .cloned()
                .collect();
            if due.is_empty() {
                return Ok((Vec::new(), Vec::new(), false));
            }
            let mut updated_names = Vec::new();
            let mut errors = Vec::new();
            let mut active_changed = false;
            for profile in due {
                let dest = match remote_config_path(&profile.id) {
                    Ok(path) => path,
                    Err(e) => {
                        errors.push(format!("{} · {e}", profile.name));
                        continue;
                    }
                };
                if let Some(parent) = dest.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        errors.push(format!("{} · {e}", profile.name));
                        continue;
                    }
                }
                let res: Result<bool, String> = async {
                    let text = download(&profile.source).await?;
                    if !prefs.singbox_path.trim().is_empty() {
                        let tmp = dest.with_extension("tmp.json");
                        std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
                        let check = config_files::run_singbox_check(
                            &prefs.singbox_path,
                            &tmp.to_string_lossy(),
                            (!prefs.working_dir.trim().is_empty())
                                .then_some(prefs.working_dir.as_str()),
                        )
                        .await;
                        let _ = std::fs::remove_file(&tmp);
                        check?;
                    }
                    config_files::replace_with_backup(&dest, text.as_bytes())
                }
                .await;
                let now = super::now_iso();
                match res {
                    Ok(changed) => {
                        let is_active = prefs.active_config_profile_id == profile.id;
                        let _ = Settings::edit(&dir, |p| {
                            if let Some(entry) =
                                p.config_profiles.iter_mut().find(|v| v.id == profile.id)
                            {
                                if changed {
                                    entry.backup_updated_at = entry.last_updated.clone();
                                }
                                entry.last_updated = now;
                                entry.last_fetch_error.clear();
                            }
                            Ok(())
                        });
                        if changed {
                            updated_names.push(profile.name);
                            if is_active {
                                active_changed = true;
                            }
                        }
                    }
                    Err(err) => {
                        let _ = Settings::edit(&dir, |p| {
                            if let Some(entry) =
                                p.config_profiles.iter_mut().find(|v| v.id == profile.id)
                            {
                                entry.last_fetch_error = err.clone();
                            }
                            Ok(())
                        });
                        errors.push(format!("{} · {err}", profile.name));
                    }
                }
            }
            Ok((updated_names, errors, active_changed))
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                this.auto_updating_profiles = false;
                if let Ok((updated, errors, active_changed)) = result {
                    if !updated.is_empty() {
                        this.message(
                            format!("已自动更新远程配置：{}", updated.join("、")),
                            false,
                            cx,
                        );
                    }
                    if active_changed {
                        this.pending_restart =
                            Some("当前使用的远程配置已自动更新，重启核心后生效。".into());
                    }
                    if !errors.is_empty() {
                        this.message(
                            format!("远程配置自动更新失败：{}", errors.join("；")),
                            true,
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn update_remote_profile(&mut self, profile: ConfigProfile, cx: &mut Context<Self>) {
        if self.config_busy.is_some() || profile.kind != "remote" {
            return;
        }
        let id = profile.id.clone();
        let name = profile.name.clone();
        self.config_busy = Some(id.clone());
        self.load_error.clear();
        let task = singboard_core::runtime().spawn(async move {
            let dir = crate::service::data_dir()?;
            let prefs = crate::maintenance::preferences()?;
            let dest = remote_config_path(&id)?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let res: Result<bool, String> = async {
                let text = download(&profile.source).await?;
                if !prefs.singbox_path.trim().is_empty() {
                    let tmp = dest.with_extension("tmp.json");
                    std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
                    let check = config_files::run_singbox_check(
                        &prefs.singbox_path,
                        &tmp.to_string_lossy(),
                        (!prefs.working_dir.trim().is_empty())
                            .then_some(prefs.working_dir.as_str()),
                    )
                    .await;
                    let _ = std::fs::remove_file(&tmp);
                    check?;
                }
                config_files::replace_with_backup(&dest, text.as_bytes())
            }
            .await;
            let now = super::now_iso();
            match res {
                Ok(changed) => {
                    let is_active = prefs.active_config_profile_id == id;
                    Settings::edit(&dir, |p| {
                        if let Some(entry) = p.config_profiles.iter_mut().find(|v| v.id == id) {
                            if changed {
                                entry.backup_updated_at = entry.last_updated.clone();
                            }
                            entry.last_updated = now;
                            entry.last_fetch_error.clear();
                        }
                        Ok(())
                    })?;
                    Ok((name, changed, is_active))
                }
                Err(err) => {
                    let _ = Settings::edit(&dir, |p| {
                        if let Some(entry) = p.config_profiles.iter_mut().find(|v| v.id == id) {
                            entry.last_fetch_error = err.clone();
                        }
                        Ok(())
                    });
                    Err(err)
                }
            }
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                this.config_busy = None;
                match result {
                    Ok((name, changed, is_active)) => {
                        this.message(if changed {
                            format!("远程配置「{name}」有新内容，已更新")
                        } else {
                            format!("远程配置「{name}」已是最新")
                        },false,cx);
                        if changed && is_active {
                            this.pending_restart =
                                Some(format!("「{name}」更新了新内容，重启核心后生效。"));
                        }
                    }
                    Err(err) => {
                        this.message(format!("更新失败，保留现有配置：{err}"),true,cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn restore_profile_backup(&mut self, profile: ConfigProfile, cx: &mut Context<Self>) {
        if self.config_busy.is_some() {
            return;
        }
        let id = profile.id.clone();
        let name = profile.name.clone();
        self.config_busy = Some(id.clone());
        let task = singboard_core::runtime().spawn_blocking(move || {
            let dir = crate::service::data_dir()?;
            let prefs = crate::maintenance::preferences()?;
            let path = profile_file_path(&profile)?;
            config_files::swap_with_backup(&path)?;
            let is_active = prefs.active_config_profile_id == id;
            let now = super::now_iso();
            Settings::edit(&dir, |p| {
                if let Some(entry) = p.config_profiles.iter_mut().find(|v| v.id == id) {
                    let prev = std::mem::replace(&mut entry.last_updated, now.clone());
                    entry.backup_updated_at = prev;
                    entry.last_fetch_error.clear();
                }
                Ok(())
            })?;
            Ok::<_, String>((name, is_active))
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = weak.update(cx, |this, cx| {
                this.config_busy = None;
                match result {
                    Ok((name, is_active)) => {
                        this.message(format!("已把「{name}」换回上一版本，再点一次可以换回来"),false,cx);
                        if is_active {
                            this.pending_restart =
                                Some(format!("「{name}」换回了上一版本，重启核心后生效。"));
                        }
                    }
                    Err(err) => {
                        this.message(format!("换回上一版本失败：{err}"),true,cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn confirm_delete_profile(&mut self, id: String, is_remote: bool, cx: &mut Context<Self>) {
        self.close_config_dialog(cx);
        if let Ok(dir) = crate::service::data_dir() {
            if is_remote && let Ok(path) = remote_config_path(&id) {
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_file(config_files::config_backup_path(&path));
            }
            if let Err(e) = Settings::edit(&dir, |p| {
                p.config_profiles.retain(|v| v.id != id);
                if p.active_config_profile_id == id {
                    p.active_config_profile_id.clear();
                }
                Ok(())
            }) {
                self.message(e,true,cx);
            } else {
                self.message("已删除配置".into(),false,cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn render_config_dialog(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let (dialog, leaving) = match (&self.config_dialog, &self.leaving_dialog) {
            (Some(dialog), _) => (dialog, false),
            (None, Some((_, dialog::LeavingDialog::Config(dialog)))) => (dialog, true),
            _ => return None,
        };
        let width = if matches!(dialog, ConfigDialog::ConfirmDelete { .. }) { 420. } else { 480. };
        let p = crate::theme::Palette::get(cx);
        let tint = self.glass_tint;
        let scrim = dialog::scrim_color(p.0);
        let dialog_card = |w: f32| {
            div()
                .w(px(w))
                .max_w_full()
                .rounded(px(22.))
                .overflow_hidden()
                .bg(p.popover(tint))
                .relative()
                .border_1()
                .border_color(p.rim())
                .shadow(crate::glass::modal_shadows(p.0))
                .child(crate::glass::card_layer(p.popover(tint), px(21.), 0.70))
                .child(crate::glass::grain_layer(px(21.)))
                .p_5()
                .flex()
                .flex_col()
                .gap_4()
        };
        let card = match dialog {
            ConfigDialog::AddLocal {
                name,
                path,
                error,
                busy,
            } => {
                let busy = *busy;
                dialog_card(480.)
                    .child(
                        div()
                            .relative()
                            .text_size(crate::theme::text_px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("添加本地配置"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().text_color(p.muted()).child("配置名称"))
                            .child(crate::glass::glass_input("cfg-local-name", name, false, p)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted())
                                    .child("配置文件路径或文件名"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            crate::glass::glass_input("cfg-local-path", path, true, p),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .id("cfg-dlg-browse")
                                            .cursor_pointer()
                                            .h(crate::theme::text_px(32.))
                                            .px_3()
                                            .rounded_full()
                                            .bg(p.fill())
                                            .hover(|s| s.bg(p.fill_strong()))
                                            .flex()
                                            .items_center()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.accent())
                                            .child("浏览")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.browse_dialog_file(window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .when(!error.is_empty(), |el| {
                        el.child(div().text_xs().text_color(p.danger()).child(error.clone()))
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("cfg-dlg-cancel")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(6.))
                                    .rounded_full()
                                    .hover(|s| s.bg(p.hover()))
                                    .text_sm()
                                    .text_color(p.muted())
                                    .child("取消")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !busy {
                                            this.close_config_dialog(cx);
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(
                                div()
                                    .id("cfg-dlg-submit")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(6.))
                                    .rounded_full()
                                    .bg(p.primary_fill())
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .child(if busy { "校验中…" } else { "添加" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_add_local(cx);
                                    })),
                            ),
                    )
            }
            ConfigDialog::AddRemote {
                name,
                url,
                interval,
                error,
                busy,
            } => {
                let busy = *busy;
                dialog_card(480.)
                    .child(
                        div()
                            .relative()
                            .text_size(crate::theme::text_px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("添加远程配置"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().text_color(p.muted()).child("配置名称"))
                            .child(crate::glass::glass_input("cfg-remote-name", name, false, p)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted())
                                    .child("远程地址（URL）"),
                            )
                            .child(crate::glass::glass_input("cfg-remote-url", url, true, p)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted())
                                    .child("自动更新间隔（小时，0 为不自动更新，最长 576）"),
                            )
                            .child(crate::glass::glass_input(
                                "cfg-remote-interval",
                                interval,
                                true,
                                p,
                            )),
                    )
                    .when(!error.is_empty(), |el| {
                        el.child(div().text_xs().text_color(p.danger()).child(error.clone()))
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("cfg-dlg-cancel")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(6.))
                                    .rounded_full()
                                    .hover(|s| s.bg(p.hover()))
                                    .text_sm()
                                    .text_color(p.muted())
                                    .child("取消")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !busy {
                                            this.close_config_dialog(cx);
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(
                                div()
                                    .id("cfg-dlg-submit")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(6.))
                                    .rounded_full()
                                    .bg(p.primary_fill())
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .child(if busy {
                                        "正在下载并校验…"
                                    } else {
                                        "添加"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_add_remote(cx);
                                    })),
                            ),
                    )
            }
            ConfigDialog::EditInfo {
                is_remote,
                name,
                source,
                interval,
                error,
                busy,
                ..
            } => {
                let busy = *busy;
                let is_remote = *is_remote;
                dialog_card(480.)
                    .child(
                        div()
                            .relative()
                            .text_size(crate::theme::text_px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("修改信息"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().text_color(p.muted()).child("名称"))
                            .child(crate::glass::glass_input("cfg-edit-name", name, false, p)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().text_color(p.muted()).child(if is_remote {
                                "远程地址"
                            } else {
                                "文件路径"
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            crate::glass::glass_input(
                                                "cfg-edit-source",
                                                source,
                                                true,
                                                p,
                                            ),
                                        ),
                                    )
                                    .when(!is_remote, |el| {
                                        el.child(
                                            div()
                                                .id("cfg-dlg-browse-edit")
                                                .cursor_pointer()
                                                .h(crate::theme::text_px(32.))
                                                .px_3()
                                                .rounded_full()
                                                .bg(p.fill())
                                                .hover(|s| s.bg(p.fill_strong()))
                                                .flex()
                                                .items_center()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(p.accent())
                                                .child("浏览")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.browse_dialog_file(window, cx);
                                                })),
                                        )
                                    }),
                            ),
                    )
                    .when_some(interval.as_ref(), |el, int_input| {
                        el.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(p.muted())
                                        .child("自动更新间隔（小时，0 为不自动更新，最长 576）"),
                                )
                                .child(crate::glass::glass_input(
                                    "cfg-edit-interval",
                                    int_input,
                                    true,
                                    p,
                                )),
                        )
                    })
                    .when(!error.is_empty(), |el| {
                        el.child(div().text_xs().text_color(p.danger()).child(error.clone()))
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("cfg-dlg-cancel")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(6.))
                                    .rounded_full()
                                    .hover(|s| s.bg(p.hover()))
                                    .text_sm()
                                    .text_color(p.muted())
                                    .child("取消")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !busy {
                                            this.close_config_dialog(cx);
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(
                                div()
                                    .id("cfg-dlg-save")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(6.))
                                    .rounded_full()
                                    .bg(p.primary_fill())
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .child(if busy { "正在校验…" } else { "保存" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_edit_info(cx);
                                    })),
                            ),
                    )
            }
            ConfigDialog::ConfirmDelete {
                id,
                name,
                is_remote,
                is_active,
            } => {
                let del_id = id.clone();
                let is_remote = *is_remote;
                let mut detail = format!("确定删除配置「{name}」？\n");
                detail.push_str(if is_remote {
                    "同时会删除已下载的远程配置文件和它的上一版本。"
                } else {
                    "仅从面板移除，不会删除本地源文件。"
                });
                if *is_active {
                    detail.push_str("\n这是当前使用的配置。删除后不会自动改用别的配置。");
                }
                dialog_card(420.)
                    .child(
                        div()
                            .relative()
                            .text_size(crate::theme::text_px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("删除配置"),
                    )
                    .child(div().text_sm().text_color(p.muted()).child(detail))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("cfg-del-cancel")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(6.))
                                    .rounded_full()
                                    .hover(|s| s.bg(p.hover()))
                                    .text_sm()
                                    .text_color(p.muted())
                                    .child("取消")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_config_dialog(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("cfg-del-confirm")
                                    .cursor_pointer()
                                    .px_4()
                                    .py(px(6.))
                                    .rounded_full()
                                    .bg(p.danger_fill())
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .child("删除")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.confirm_delete_profile(del_id.clone(), is_remote, cx);
                                    })),
                            ),
                    )
            }
        };

        Some(dialog::surface(
            "config-pop",
            div()
                .id("cfg-dialog-scrim")
                .absolute()
                .inset_0()
                .size_full()
                .bg(scrim)
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            card, width, leaving, cx,
        ))
    }

    pub(super) fn config_page(&self, _: &Window, cx: &Context<Self>) -> Div {
        if let Some(editor) = &self.config_editor {
            return div()
                .size_full()
                .px(px(20.))
                .pb(px(16.))
                .child(editor.clone());
        }
        let palette = crate::theme::Palette::get(cx);
        let translucent = true;
        let tint = self.glass_tint;
        let prefs = crate::maintenance::preferences().unwrap_or_default();
        let active_id = prefs.active_config_profile_id.clone();
        let profiles = prefs.config_profiles.clone();
        let has_no_active = !profiles.is_empty() && !profiles.iter().any(|p| p.id == active_id);
        let owner = cx.weak_entity();

        let glass_action = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .relative()
                .overflow_hidden()
                .cursor_pointer()
                .rounded_full()
                .bg(palette.fill())
                .border_1()
                .border_color(palette.control_rim())
                .shadow_sm()
                .child(crate::glass::control_layer(palette, px(999.), true))
                .child(
                    div()
                        .relative()
                        .px_3()
                        .py(px(6.))
                        .rounded_full()
                        .hover(|s| s.bg(palette.fill_strong()))
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.accent())
                        .child(label),
                )
        };

        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                crate::glass::backdrop_heading("配置", palette, cx)
                    .text_size(crate::theme::text_px(26.))
                    .line_height(crate::theme::text_px(32.))
                    .font_weight(FontWeight::BOLD),
            )
            .when(!profiles.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(glass_action("cfg-add-local", "添加本地配置").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.open_add_local_dialog(window, cx);
                            }),
                        ))
                        .child(glass_action("cfg-add-remote", "添加远程配置").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.open_add_remote_dialog(window, cx);
                            }),
                        )),
                )
            });

        let mut grid = div()
            .w_full()
            .grid()
            .grid_cols(if self.config_two_columns && !profiles.is_empty() {
                2
            } else {
                1
            })
            .items_start()
            .gap_3();
        for (idx, profile) in profiles.iter().enumerate() {
            let is_active = profile.id == active_id;
            let is_remote = profile.kind == "remote";
            let is_busy = self.config_busy.as_deref() == Some(profile.id.as_str());
            let backup_ready = has_backup(profile);
            let card_border = if is_active {
                palette.selection_border()
            } else {
                palette.rim()
            };

            let status_btn = if is_active {
                div()
                    .flex_none()
                    .h(crate::theme::text_px(24.))
                    .px(px(10.))
                    .rounded_full()
                    .bg(palette.primary_soft())
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.accent())
                    .child(svg().path("profile-active").size(px(12.)).text_color(palette.accent()))
                    .child("使用中")
                    .into_any_element()
            } else {
                let p_select = profile.clone();
                div()
                    .id(("cfg-select", idx))
                    .flex_none()
                    .cursor_pointer()
                    .h(crate::theme::text_px(24.))
                    .px_3()
                    .rounded_full()
                    .bg(palette.fill())
                    .border_1()
                    .border_color(palette.control_rim())
                    .hover(|s| s.bg(palette.fill_strong()))
                    .flex()
                    .items_center()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.accent())
                    .child(if is_busy { "校验中…" } else { "使用" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_profile(p_select.clone(), cx);
                    }))
                    .into_any_element()
            };

            let mut meta_parts = Vec::new();
            let stamp = short_timestamp(&profile.last_updated);
            if !stamp.is_empty() {
                meta_parts.push(format!(
                    "{} {stamp}",
                    if is_remote { "更新于" } else { "保存于" }
                ));
            }
            if is_remote && profile.auto_update_interval > 0 {
                meta_parts.push(format!("每 {} 小时自动更新", profile.auto_update_interval));
            }
            let meta_line = meta_parts.join(" · ");

            let p_edit = profile.clone();
            let mut footer =
                div()
                    .pt_2()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        div()
                            .id(("cfg-edit", idx))
                            .cursor_pointer()
                            .px_2()
                            .py(px(3.))
                            .rounded(px(8.))
                            .hover(|s| s.bg(palette.hover()))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .child("编辑")
                            .on_click(cx.listener(
                                move |this, _, window, cx| match profile_file_path(&p_edit) {
                                    Ok(path) => {
                                        this.edit(path, p_edit.name.clone(), is_remote, window, cx)
                                    }
                                    Err(e) => {
                                        this.message(e,true,cx);
                                        cx.notify();
                                    }
                                },
                            )),
                    );
            if is_remote {
                let p_upd = profile.clone();
                footer = footer.child(
                    div()
                        .id(("cfg-update", idx))
                        .cursor_pointer()
                        .px_2()
                        .py(px(3.))
                        .rounded(px(8.))
                        .hover(|s| s.bg(palette.hover()))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .child(if is_busy { "更新中…" } else { "更新" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_remote_profile(p_upd.clone(), cx);
                        })),
                );
            }
            if backup_ready {
                let p_bak = profile.clone();
                let bak_stamp = short_timestamp(&profile.backup_updated_at);
                let bak_label = if bak_stamp.is_empty() {
                    "换回上一版本".into()
                } else {
                    format!("换回 {bak_stamp} 的版本")
                };
                footer = footer.child(
                    div()
                        .id(("cfg-restore", idx))
                        .cursor_pointer()
                        .px_2()
                        .py(px(3.))
                        .rounded(px(8.))
                        .hover(|s| s.bg(palette.hover()))
                        .text_xs()
                        .text_color(palette.muted())
                        .child(bak_label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.restore_profile_backup(p_bak.clone(), cx);
                        })),
                );
            }
            let p_info = profile.clone();
            let del_id = profile.id.clone();
            let del_name = profile.name.clone();
            footer = footer
                .child(
                    div()
                        .id(("cfg-info", idx))
                        .cursor_pointer()
                        .px_2()
                        .py(px(3.))
                        .rounded(px(8.))
                        .hover(|s| s.bg(palette.hover()))
                        .text_xs()
                        .text_color(palette.muted())
                        .child("修改信息")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_edit_info_dialog(&p_info, window, cx);
                        })),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .id(("cfg-delete", idx))
                        .cursor_pointer()
                        .px_2()
                        .py(px(3.))
                        .rounded(px(8.))
                        .hover(|s| s.bg(palette.danger_soft()))
                        .text_xs()
                        .text_color(palette.danger())
                        .child("删除")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.config_dialog = Some(ConfigDialog::ConfirmDelete {
                                id: del_id.clone(),
                                name: del_name.clone(),
                                is_remote,
                                is_active,
                            });
                            cx.notify();
                        })),
                );

            let card = glass_card(palette, translucent, tint)
                .w_full()
                .min_w_0()
                .p_4()
                .border_color(card_border)
                .flex()
                .flex_col()
                .gap_1()
                .when(is_active, |el| {
                    el.child(div().absolute().inset_0().rounded(px(17.)).bg(palette.primary_fill().opacity(0.10)))
                })
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_size(crate::theme::text_px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(crate::emoji::text(profile.name.clone())),
                        )
                        .child(status_btn),
                )
                .child(
                    div()
                        .pt(px(2.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex_none()
                                .h(crate::theme::text_px(20.))
                                .px_2()
                                .rounded_full()
                                .bg(palette.fill())
                                .flex()
                                .items_center()
                                .text_size(crate::theme::text_px(11.))
                                .text_color(palette.muted())
                                .gap_1()
                                .child(
                                    svg()
                                        .path(if is_remote {
                                            "profile-remote"
                                        } else {
                                            "folder"
                                        })
                                        .size(px(12.))
                                        .text_color(palette.muted()),
                                )
                                .child(if is_remote { "远程" } else { "本地" }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_family("Consolas")
                                .text_size(crate::theme::text_px(11.5))
                                .text_color(palette.muted())
                                .truncate()
                                .child(source_label(&profile.kind, &profile.source)),
                        ),
                )
                .when(!meta_line.is_empty(), |el| {
                    el.child(div().text_xs().text_color(palette.muted()).child(meta_line))
                })
                .when(!profile.last_fetch_error.is_empty(), |el| {
                    let first = profile
                        .last_fetch_error
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or(&profile.last_fetch_error);
                    el.child(
                        div()
                            .text_xs()
                            .text_color(palette.danger())
                            .truncate()
                            .child(format!("保留现有配置 · 更新失败：{first}")),
                    )
                })
                .child(footer);

            grid = grid.child(card);
        }

        let scroll = self.scroll_pane(
            "config-scroll",
            div().w_full().pb_6().child(if profiles.is_empty() {
                div()
                    .w_full()
                    .py_12()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("还没有配置"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted())
                            .child("添加本地的 sing-box 配置文件，或者填入订阅地址下载远程配置。"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(glass_action("cfg-empty-local", "添加本地配置").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.open_add_local_dialog(window, cx);
                                }),
                            ))
                            .child(glass_action("cfg-empty-remote", "添加远程配置").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.open_add_remote_dialog(window, cx);
                                }),
                            )),
                    )
            } else {
                grid
            }),
            cx,
        );

        let top = div()
            .w_full()
            .px(px(20.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .child(header)
            .when_some(self.pending_restart.clone(), |el, text| {
                el.child(
                    glass_card(palette, translucent, tint)
                        .w_full()
                        .px_3()
                        .py(px(6.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .child(text)
                        .child(
                            div()
                                .id("cfg-restart-now")
                                .cursor_pointer()
                                .px_2()
                                .py(px(2.))
                                .rounded(px(6.))
                                .hover(|s| s.bg(palette.hover()))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.accent())
                                .child("立即重启核心")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.pending_restart = None;
                                    this.tray_service("restart", cx);
                                })),
                        )
                        .child(
                            div()
                                .id("cfg-restart-later")
                                .cursor_pointer()
                                .px_2()
                                .py(px(2.))
                                .rounded(px(6.))
                                .hover(|s| s.bg(palette.hover()))
                                .text_color(palette.muted())
                                .child("稍后")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.pending_restart = None;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when(has_no_active, |el| {
                el.child(
                    glass_card(palette, translucent, tint)
                        .w_full()
                        .px_3()
                        .py(px(6.))
                        .text_xs()
                        .text_color(palette.muted())
                        .child("还没有选用配置，核心使用的是上次写入的运行配置。在下面选一份点「使用」。"),
                )
            })
            .when(!self.load_error.is_empty(), |el| {
                el.child(
                    div()
                        .px_1()
                        .text_xs()
                        .text_color(palette.muted())
                        .child(self.load_error.clone()),
                )
            });

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(top)
            .child(div().w_full().flex_1().min_h_0().on_children_prepainted(move |bounds, _, cx| {
                if let Some(bounds) = bounds.first() {
                    let two_columns = bounds.size.width >= px(680.);
                    if owner.upgrade().is_some_and(|view| view.read(cx).config_two_columns != two_columns) {
                        let owner = owner.clone();
                        cx.defer(move |cx| {
                            let _ = owner.update(cx, |view, cx| {
                                view.config_two_columns = two_columns;
                                cx.notify();
                            });
                        });
                    }
                }
            }).flex().flex_col().child(scroll))

    }
}
