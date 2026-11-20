use crate::{service, theme::Palette};
use gpui::{prelude::*, *};
use singboard_stream::{profiles::Store, settings::Settings, update};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const APPLY: &str = "--apply-native-update";
pub const CLEANUP: &str = "--finish-native-update";

pub fn preferences() -> Result<Settings, String> {
    Settings::load(&service::data_dir()?)
}

fn read_legacy(dir: &Path) -> Result<Settings, String> {
    let snapshot = tempfile::tempdir().map_err(|e| e.to_string())?;
    for entry in fs::read_dir(dir).map_err(|_| "未找到旧版数据，可选择设置备份文件")?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if s == "CURRENT"
            || s.starts_with("MANIFEST-")
            || s.ends_with(".ldb")
            || s.ends_with(".sst")
            || s.ends_with(".log")
        {
            fs::copy(entry.path(), snapshot.path().join(&name))
                .map_err(|_| "无法复制旧版数据库，请关闭旧版后重试")?;
        }
    }
    let mut db = rusty_leveldb::DB::open(
        snapshot.path(),
        rusty_leveldb::Options {
            create_if_missing: false,
            ..Default::default()
        },
    )
    .map_err(|_| "无法读取旧版数据库，请关闭旧版后重试或选择备份文件")?;
    for origin in [
        "http://tauri.localhost",
        "https://tauri.localhost",
        "tauri://localhost",
    ] {
        let key = format!("_{origin}\0\x01singboard-config");
        if let Some(bytes) = db.get(key.as_bytes()) {
            let (encoding, value) = bytes.split_first().ok_or("旧版设置为空")?;
            let text = match encoding {
                1 => value.iter().map(|&b| char::from(b)).collect(),
                0 if value.len() % 2 == 0 => String::from_utf16(
                    &value
                        .chunks_exact(2)
                        .map(|v| u16::from_le_bytes([v[0], v[1]]))
                        .collect::<Vec<_>>(),
                )
                .map_err(|_| "无法解码旧版设置")?,
                _ => return Err("无法识别旧版设置编码".into()),
            };
            return Settings::parse(&text);
        }
    }
    Err("旧版数据库中没有 singboard-config，请选择备份文件".into())
}

fn installed_settings() -> Result<Settings, String> {
    let root = std::env::var_os("LOCALAPPDATA").ok_or("无法定位旧版数据")?;
    read_legacy(&PathBuf::from(root).join("singboard/EBWebView/Default/Local Storage/leveldb"))
}

pub fn launch_installer(source: &Path, digest: &str) -> Result<(), String> {
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    let worker = source.with_file_name("singboard-updater.exe");
    fs::copy(&target, &worker).map_err(|e| e.to_string())?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut command = Command::new(worker);
    command
        .arg(APPLY)
        .arg(&target)
        .arg(std::process::id().to_string())
        .arg(digest);
    if let Some(path) = args
        .iter()
        .position(|a| a == "--wallpaper")
        .and_then(|i| args.get(i + 1))
    {
        command.arg("--wallpaper").arg(path);
    }
    command.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn apply(
    target: &Path,
    pid: u32,
    digest: &str,
    restart_args: &[std::ffi::OsString],
) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject},
    };
    let worker = std::env::current_exe().map_err(|e| e.to_string())?;
    if digest.len() != 64
        || !digest.bytes().all(|c| c.is_ascii_hexdigit())
        || worker.parent() != Some(service::data_dir()?.join("updates").join(digest).as_path())
        || !target.is_absolute()
    {
        return Err("更新目录或校验信息无效".into());
    }
    let staged = worker.with_file_name(update::asset_name());
    let handle = unsafe { OpenProcess(0x00100000, 0, pid) };
    if !handle.is_null() {
        let status = unsafe {
            let result = WaitForSingleObject(handle, 60000);
            CloseHandle(handle);
            result
        };
        if status != WAIT_OBJECT_0 {
            return Err("面板仍在运行，已取消替换".into());
        }
    }
    let before = fs::read(&worker).map_err(|e| e.to_string())?;
    let content = fs::read(&staged).map_err(|e| e.to_string())?;
    if let Err(e) = replace_update(target, &before, &content, digest) {
        if fs::read(target).ok().as_ref() == Some(&before) {
            let _ = Command::new(target).args(restart_args).spawn();
        }
        return Err(format!("更新失败：{e}\n下载位置：{}", staged.display()));
    }
    if let Err(e) = Command::new(target)
        .args(restart_args)
        .arg(CLEANUP)
        .arg(digest)
        .spawn()
    {
        singboard_stream::config_files::replace_checked(target, &before, Some(&content))
            .map_err(|error| format!("启动失败：{e}。恢复旧版失败：{error}"))?;
        Command::new(target)
            .args(restart_args)
            .spawn()
            .map_err(|error| format!("已恢复旧版，请手动启动：{error}"))?;
        return Err(format!("新版启动失败，已恢复旧版：{e}"));
    }
    let _ = fs::remove_file(staged);
    Ok(())
}

pub fn finish(digest: &str) {
    if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
        return;
    }
    let Ok(root) = service::data_dir() else {
        return;
    };
    let dir = root.join("updates").join(digest);
    let worker = dir.join("singboard-updater.exe");
    std::thread::spawn(move || {
        if !std::env::current_exe()
            .ok()
            .and_then(|p| update::hash(&p).ok())
            .is_some_and(|hash| dir.file_name() == Some(hash.as_ref()))
        {
            return;
        }
        for _ in 0..20 {
            match fs::remove_file(&worker) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(250)),
            }
        }
        let _ = fs::remove_dir(dir);
    });
}

fn replace_update(
    target: &Path,
    before: &[u8],
    content: &[u8],
    digest: &str,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    if format!("{:x}", Sha256::digest(content)) != digest {
        return Err("下载的 EXE 已发生变化".into());
    }
    singboard_stream::config_files::replace_checked(target, content, Some(before)).map(|_| ())
}

enum ResultView {
    Preview(Settings),
    Imported(String),
}
struct Maintenance {
    prefs: Settings,
    pending: Option<Settings>,
    message: String,
    busy: bool,
}

pub fn open(cx: &mut App) -> Result<(), String> {
    for handle in cx.windows() {
        if let Some(window) = handle.downcast::<Maintenance>()
            && window.update(cx, |_, w, _| w.activate_window()).is_ok()
        {
            return Ok(());
        }
    }
    let prefs = preferences()?;
    let bounds = Bounds::centered(None, size(px(620.), px(370.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("导入旧版设置".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |_, cx| {
            cx.new(|_| Maintenance {
                prefs,
                pending: None,
                message: String::new(),
                busy: false,
            })
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

impl Maintenance {
    fn job(
        &mut self,
        work: impl Future<Output = Result<ResultView, String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message = "正在处理…".into();
        let task = singboard_stream::runtime().spawn(work);
        cx.spawn(async move |weak, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = weak.update(cx, |this, cx| {
                this.busy = false;
                this.message = match result {
                    Err(e) => e,
                    Ok(ResultView::Preview(data)) => {
                        let msg = data.preview();
                        this.pending = Some(data);
                        msg
                    }
                    Ok(ResultView::Imported(msg)) => {
                        this.pending = None;
                        if let Ok(p) = preferences() {
                            this.prefs = p;
                        }
                        msg
                    }
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn choose(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        let dialog = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择旧版设置 JSON".into()),
        });
        cx.spawn(async move |weak, cx| {
            if let Ok(Ok(Some(paths))) = dialog.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = weak.update(cx, |this, cx| {
                    this.job(
                        async move {
                            if fs::metadata(&path).map_err(|e| e.to_string())?.len()
                                > 16 * 1024 * 1024
                            {
                                return Err("设置文件超过 16 MiB".into());
                            }
                            Ok(ResultView::Preview(Settings::parse(
                                &fs::read_to_string(path).map_err(|e| e.to_string())?,
                            )?))
                        },
                        cx,
                    )
                });
            }
        })
        .detach();
    }
    fn button(
        &self,
        label: &'static str,
        cx: &Context<Self>,
        act: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        div()
            .id(label)
            .px_3()
            .py_1()
            .rounded_md()
            .bg(Palette::get(cx).fill())
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.busy {
                    act(this, cx)
                }
            }))
    }
}

impl Render for Maintenance {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut imports = div()
            .flex()
            .gap_2()
            .child(self.button("读取本机旧版", cx, |this, cx| {
                this.pending = None;
                this.job(async { Ok(ResultView::Preview(installed_settings()?)) }, cx)
            }))
            .child(self.button("选择备份文件", cx, Self::choose));
        if self.pending.is_some() {
            imports = imports
                .child(self.button("取消", cx, |this, cx| {
                    this.pending = None;
                    this.message.clear();
                    cx.notify();
                }))
                .child(self.button("确认导入", cx, |this, cx| {
                    let settings = this.pending.as_ref().unwrap().clone();
                    this.job(
                        async move {
                            let dir = service::data_dir()?;
                            let result = settings
                                .import(&dir, &Store::new(dir.join("Native Subscriptions")))?;
                            Ok(ResultView::Imported(format!(
                                "已导入，新增 {} 个订阅。当前后端与主题已应用。",
                                result["added"]
                            )))
                        },
                        cx,
                    );
                }));
        }
        let p = Palette::get(cx);
        let mut page = div()
            .id("maintenance")
            .overflow_y_scroll()
            .size_full()
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .bg(p.page())
            .text_color(p.ink())
            .child("导入旧版设置")
            .child(imports)
            .child(self.message.clone());
        if self.prefs.clash_apis.len() > 1 {
            page = page.child("API 后端");
            for (i, api) in self.prefs.clash_apis.iter().enumerate() {
                let id = api.id.clone();
                let selected = self.prefs.active().is_some_and(|p| p.id == id);
                page = page.child(
                    div()
                        .id(("endpoint", i))
                        .cursor_pointer()
                        .child(format!("{}{}", if selected { "✓ " } else { "" }, api.name))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = id.clone();
                            this.job(
                                async move {
                                    Settings::select_endpoint(&service::data_dir()?, &id)?;
                                    Ok(ResultView::Imported("已切换后端".into()))
                                },
                                cx,
                            );
                        })),
                );
            }
        }
        page
    }
}

#[cfg(test)]
mod tests {
    use super::{read_legacy, replace_update};
    use std::fs;
    #[test]
    fn opens_chromium_utf16_snapshot_while_the_source_is_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = rusty_leveldb::DB::open(dir.path(), Default::default()).unwrap();
        let text = r#"{"clashApis":[{"name":"家中","url":"http://127.0.0.1:9090"}]}"#;
        let mut value = vec![0];
        value.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let key = b"_http://tauri.localhost\0\x01singboard-config";
        db.put(key, &value).unwrap();
        db.flush().unwrap();
        assert_eq!(
            read_legacy(dir.path()).unwrap().active().unwrap().name,
            "家中"
        );
        assert_eq!(&db.get(key).unwrap()[..], &value);
    }
    #[test]
    fn installation_preserves_the_old_executable_on_mismatch() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("singboard.exe");
        fs::write(&exe, b"old").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"new"));
        assert!(replace_update(&exe, b"old", b"wrong", &digest).is_err());
        assert!(replace_update(&exe, b"stale", b"new", &digest).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"old");
        replace_update(&exe, b"old", b"new", &digest).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert_eq!(fs::read(exe.with_extension("exe.bak")).unwrap(), b"old");
    }
}
