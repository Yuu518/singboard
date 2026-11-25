use crate::theme::Palette;
use gpui::{prelude::*, *};
use gpui_component::input::{Editor as CodeEditor, EditorState, InputEvent, Redo, Undo};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use singboard_core::config_files as files;

actions!(config_editor, [SaveConfig, ToggleComment]);

#[derive(Default)]
struct OpenEditor(Option<(AnyWindowHandle, WeakEntity<Editor>)>);
impl Global for OpenEditor {}

pub fn has_pending_changes(cx: &App) -> bool {
    cx.global::<OpenEditor>()
        .0
        .as_ref()
        .and_then(|(_, editor)| editor.upgrade())
        .is_some_and(|e| {
            let editor = e.read(cx);
            editor.dirty || editor.busy
        })
}

pub fn init(cx: &mut App) {
    cx.set_global(OpenEditor::default());
    cx.bind_keys([
        KeyBinding::new("ctrl-s", SaveConfig, Some("ConfigEditor")),
        KeyBinding::new("ctrl-/", ToggleComment, Some("ConfigEditor")),
    ]);
}

pub fn quit(cx: &mut App) {
    if let Some((window, editor)) = cx.global::<OpenEditor>().0.clone() {
        if window
            .update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| editor.request(Pending::Quit, window, cx))
            })
            .is_ok_and(|result| result.is_ok())
        {
            return;
        }
    }
    cx.quit();
}

#[derive(Clone)]
pub struct Core {
    pub executable: PathBuf,
    pub working_dir: PathBuf,
}

#[derive(Clone)]
struct Document {
    path: PathBuf,
    saved: String,
    text: String,
}

impl Document {
    fn open(path: PathBuf) -> Result<Self, String> {
        let saved = std::fs::read_to_string(&path).map_err(|e| format!("无法读取配置：{e}"))?;
        Ok(Self::new(path, saved))
    }

    fn new(path: PathBuf, saved: String) -> Self {
        let text = saved
            .strip_prefix('\u{feff}')
            .unwrap_or(&saved)
            .replace("\r\n", "\n");
        Self { path, saved, text }
    }

    fn encode(&self, text: &str) -> String {
        if self.text == text {
            return self.saved.clone();
        }
        let bom = if self.saved.starts_with('\u{feff}') {
            "\u{feff}"
        } else {
            ""
        };
        let body = if self.saved.contains("\r\n") {
            text.replace('\n', "\r\n")
        } else {
            text.to_owned()
        };
        format!("{bom}{body}")
    }

    fn save(&self, content: &str) -> Result<(), String> {
        files::replace_checked(&self.path, content.as_bytes(), Some(self.saved.as_bytes()))
            .map(|_| ())
    }
}

pub(crate) async fn validate(
    path: &Path,
    text: &str,
    core: Option<&Core>,
) -> Result<&'static str, String> {
    singboard_core::profiles::validate_jsonc(text)?;
    let Some(core) = core else {
        return Ok("JSONC 语法通过，尚未校验核心配置");
    };
    let mut temp = tempfile::Builder::new()
        .prefix(".singboard-check-")
        .suffix(".json")
        .tempfile_in(path.parent().ok_or("配置目录无效")?)
        .map_err(|e| e.to_string())?;
    temp.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    let temp = temp.into_temp_path();
    let executable = core.executable.to_string_lossy();
    let config = temp.to_string_lossy();
    let dir = core.working_dir.to_string_lossy();
    tokio::time::timeout(
        Duration::from_secs(30),
        files::run_singbox_check(&executable, &config, Some(&dir)),
    )
    .await
    .map_err(|_| "sing-box 校验超过 30 秒".to_string())?
    .map_err(|error| error.replace(config.as_ref(), &path.display().to_string()))?;
    Ok("sing-box 校验通过")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    Close,
    Quit,
    Backup,
}

pub struct Editor {
    inline: Option<InlineEditor>,
    doc: Document,
    loaded_backup: Option<Document>,
    input: Entity<EditorState>,
    prefix: String,
    suffix: String,
    module: String,
    choices: Vec<(String, std::ops::Range<usize>)>,
    core: Option<Core>,
    dirty: bool,
    busy: bool,
    pending: Option<Pending>,
    status: String,
}

struct InlineEditor {
    owner: WeakEntity<crate::workspace::Workspace>,
    name: String,
    remote: bool,
}

pub fn inline(
    path: PathBuf,
    core: Option<Core>,
    name: String,
    remote: bool,
    owner: WeakEntity<crate::workspace::Workspace>,
    window: &mut Window,
    cx: &mut App,
) -> Result<Entity<Editor>, String> {
    if let Some((handle, existing)) = cx.global::<OpenEditor>().0.clone()
        && existing.upgrade().is_some()
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return Err("请先关闭当前配置编辑器，再打开其他文件".into());
    }
    let doc = Document::open(path)?;
    Ok(create_editor(
        doc,
        core,
        Some(InlineEditor {
            owner,
            name,
            remote,
        }),
        window,
        cx,
    ))
}

fn create_editor(
    doc: Document,
    core: Option<Core>,
    inline: Option<InlineEditor>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let input = cx.new(|cx| {
        EditorState::new(window, cx)
            .language("json")
            .line_number(true)
            .default_value(doc.text.clone())
    });
    let editor = cx.new(|cx| {
        cx.subscribe(
            &input,
            |editor: &mut Editor, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = format!(
                        "{}{}{}",
                        editor.prefix,
                        input.read(cx).value(),
                        editor.suffix
                    );
                    editor.dirty = match &editor.loaded_backup {
                        Some(backup) if text == backup.text.as_str() => {
                            backup.saved != editor.doc.saved
                        }
                        _ => text != editor.doc.text.as_str(),
                    };
                    editor.status.clear();
                    cx.notify();
                }
            },
        )
        .detach();
        Editor {
            inline,
            doc,
            loaded_backup: None,
            input: input.clone(),
            prefix: String::new(),
            suffix: String::new(),
            module: String::new(),
            choices: vec![],
            core,
            status: String::new(),
            busy: false,
            dirty: false,
            pending: None,
        }
    });
    let weak = editor.downgrade();
    cx.global_mut::<OpenEditor>().0 = Some((window.window_handle(), weak.clone()));
    window.focus(&input.focus_handle(cx), cx);
    editor
}

impl Editor {
    fn check(&mut self, save: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.pending = None;
        self.status = "正在校验…".into();
        self.busy = true;
        let doc = self.doc.clone();
        let content = self
            .loaded_backup
            .as_ref()
            .unwrap_or(&doc)
            .encode(&self.full_text(cx));
        let core = self.core.clone();
        let job = singboard_core::runtime().spawn(async move {
            let message = validate(&doc.path, &content, core.as_ref()).await?;
            if save {
                doc.save(&content)?;
            }
            Ok::<_, String>((content, message))
        });
        cx.spawn(async move |weak, cx| {
            let result = job.await.map_err(|e| e.to_string()).and_then(|v| v);
            let _ = weak.update(cx, |editor, cx| {
                editor.busy = false;
                editor.status = match result {
                    Ok((content, message)) if save => {
                        editor.doc = Document::new(editor.doc.path.clone(), content);
                        editor.loaded_backup = None;
                        editor.dirty = editor.full_text(cx) != editor.doc.text;
                        format!("已保存 · {message}")
                    }
                    Ok((_, message)) => message.into(),
                    Err(e) => e,
                };
                if let Some(pending) = editor.pending {
                    if !editor.dirty {
                        editor.pending = None;
                        match pending {
                            Pending::Quit => {
                                cx.global_mut::<OpenEditor>().0 = None;
                                cx.quit();
                            }
                            Pending::Close => {
                                if let Some(inline) = &editor.inline {
                                    cx.global_mut::<OpenEditor>().0 = None;
                                    let _ = inline.owner.update(cx, |owner, cx| owner.close_editor(cx));
                                }
                            }
                            Pending::Backup => {}
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn full_text(&self, cx: &App) -> String {
        format!(
            "{}{}{}",
            self.prefix,
            self.input.read(cx).value(),
            self.suffix
        )
    }
    fn select_module(
        &mut self,
        name: String,
        range: std::ops::Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let full = self.full_text(cx);
        let Some(value) = full.get(range.clone()) else {
            return;
        };
        self.prefix = full[..range.start].into();
        self.suffix = full[range.end..].into();
        self.module = name;
        self.input
            .update(cx, |f, cx| f.set_value(value.to_owned(), window, cx));
        cx.notify();
    }

    fn switch_module(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let full = self.full_text(cx);
        if target.is_empty() {
            self.choices.clear();
            self.select_module(String::new(), 0..full.len(), window, cx);
            return;
        }
        match singboard_core::editor::modules(&full) {
            Ok(list) => {
                if let Some((name, range)) = list.iter().find(|(k, _)| k == target).cloned() {
                    self.choices = list;
                    self.select_module(name, range, window, cx);
                }
            }
            Err(e) => {
                self.status = e;
                cx.notify();
            }
        }
    }
    fn toggle_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let text = self.input.read(cx).value();
        let selection = self.input.read(cx).selected_range();
        let (mut value, _) = singboard_core::editor::toggle_comment(&text, selection);
        if !self.suffix.is_empty()
            && !self.suffix.starts_with('\n')
            && !self.suffix.starts_with("\r\n")
            && !value.ends_with('\n')
            && value
                .rsplit('\n')
                .next()
                .is_some_and(|line| line.trim_start().starts_with("//"))
        {
            value.push('\n');
        }
        self.input
            .update(cx, |f, cx| f.replace_all(value, window, cx));
        cx.notify();
    }

    fn choose_core(&mut self, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择 sing-box 可执行文件".into()),
        });
        cx.spawn(async move |weak, cx| {
            let result = picker.await;
            let _ = weak.update(cx, |editor, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            let cwd = editor
                                .doc
                                .path
                                .parent()
                                .unwrap_or(Path::new("."))
                                .to_owned();
                            editor.core = Some(Core {
                                executable: path,
                                working_dir: cwd,
                            });
                            editor.status.clear();
                        }
                    }
                    Ok(Err(e)) => editor.status = e.to_string(),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn request(&mut self, action: Pending, window: &mut Window, cx: &mut Context<Self>) {
        window.activate_window();
        if let Some(inline) = &self.inline {
            let _ = inline.owner.update(cx, |owner, cx| {
                owner.open(crate::workspace::Page::Config, cx)
            });
        }
        if self.busy {
            self.pending = Some(action);
            cx.notify();
            return;
        }
        if self.dirty {
            self.pending = Some(action);
            cx.notify();
        } else {
            self.finish(action, window, cx);
        }
    }

    fn finish(&mut self, action: Pending, window: &mut Window, cx: &mut Context<Self>) {
        self.pending = None;
        match action {
            Pending::Close => {
                if let Some(inline) = &self.inline {
                    cx.global_mut::<OpenEditor>().0 = None;
                    let _ = inline.owner.update(cx, |owner, cx| owner.close_editor(cx));
                } else {
                    window.remove_window();
                }
            }
            Pending::Quit => cx.quit(),
            Pending::Backup => match Document::open(files::config_backup_path(&self.doc.path)) {
                Ok(backup) => {
                    let text = backup.text.clone();
                    self.prefix.clear();
                    self.suffix.clear();
                    self.module.clear();
                    self.choices.clear();
                    self.dirty = backup.saved != self.doc.saved;
                    self.loaded_backup = Some(backup);
                    self.input
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                    self.status = "已载入上一版，保存后生效".into();
                }
                Err(e) => self.status = e,
            },
        }
        cx.notify();
    }
}

fn ghost_btn(id: &'static str, label: &'static str, palette: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .h(crate::theme::text_px(28.))
        .px_2()
        .rounded(px(8.))
        .flex()
        .items_center()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette.ink())
        .hover(|s| s.bg(palette.fill()))
        .cursor_pointer()
        .child(label)
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::get(cx);
        let tint = crate::glass::current_tint(cx);
        let in_module_mode = !self.module.is_empty() || !self.choices.is_empty();
        let can_save = self.dirty && !self.busy;

        let mut bar = div().flex().flex_wrap().items_center().gap_1();
        if let Some(pending) = self.pending {
            let label = if pending == Pending::Backup {
                "放弃编辑并载入上一版"
            } else {
                "放弃编辑并关闭"
            };
            bar = bar
                .gap_2()
                .child(div().text_xs().text_color(palette.muted()).child("有未保存的更改"))
                .child(ghost_btn("cancel", "继续编辑", palette).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.pending = None;
                        cx.notify();
                    },
                )))
                .child(
                    div()
                        .id("discard")
                        .h(crate::theme::text_px(28.))
                        .px_3()
                        .rounded_full()
                        .bg(palette.danger_soft())
                        .flex()
                        .items_center()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.danger())
                        .cursor_pointer()
                        .child(label)
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.finish(pending, window, cx)),
                        ),
                );
        } else {
            bar = bar
                .child(ghost_btn("undo", "撤销", palette).on_click(cx.listener(
                    |this, _, window, cx| {
                        if !this.busy {
                            window.focus(&this.input.focus_handle(cx), cx);
                            window.dispatch_action(Box::new(Undo), cx);
                        }
                    },
                )))
                .child(ghost_btn("redo", "重做", palette).on_click(cx.listener(
                    |this, _, window, cx| {
                        if !this.busy {
                            window.focus(&this.input.focus_handle(cx), cx);
                            window.dispatch_action(Box::new(Redo), cx);
                        }
                    },
                )))
                .child(ghost_btn("format", "格式化", palette).on_click(cx.listener(
                    |this, _, w, cx| {
                        if this.busy {
                            return;
                        }
                        match singboard_core::editor::format(&this.input.read(cx).value()) {
                            Ok(text) => this.input.update(cx, |f, cx| f.replace_all(text, w, cx)),
                            Err(e) => this.status = e,
                        }
                        cx.notify();
                    },
                )))
                .child(
                    ghost_btn("comment", "注释", palette)
                        .on_click(cx.listener(|this, _, w, cx| this.toggle_comment(w, cx))),
                )
                .child(
                    ghost_btn(
                        "validate",
                        if self.busy { "校验中…" } else { "校验" },
                        palette,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.check(false, cx))),
                )
                .child(
                    div()
                        .id("save")
                        .h(crate::theme::text_px(28.))
                        .px(px(if can_save { 10. } else { 8. }))
                        .rounded(px(if can_save { 999. } else { 8. }))
                        .flex()
                        .items_center()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .when(can_save, |d| {
                            d.cursor_pointer()
                                .bg(palette.primary_fill())
                                .text_color(rgb(0xffffff))
                                .on_click(cx.listener(|this, _, _, cx| this.check(true, cx)))
                        })
                        .when(!can_save, |d| {
                            d.text_color(palette.muted()).opacity(0.55)
                        })
                        .child("保存"),
                )
                .when(files::config_backup_path(&self.doc.path).is_file(), |bar| {
                    bar.child(ghost_btn("backup", "载入上一版", palette).on_click(
                        cx.listener(|this, _, window, cx| {
                            this.request(Pending::Backup, window, cx)
                        }),
                    ))
                })
                .when(self.inline.is_none(), |bar| {
                    bar.child(ghost_btn("core", "选择核心", palette).on_click(cx.listener(
                        |this, _, _, cx| {
                            if !this.busy {
                                this.choose_core(cx);
                            }
                        },
                    )))
                });
        }

        let mode_tabs = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded_full()
            .bg(palette.segment_track())
            .border_1()
            .border_color(palette.control_rim())
            .child(
                div()
                    .id("editor-mode-whole")
                    .cursor_pointer()
                    .px(px(10.))
                    .py(px(2.))
                    .rounded_full()
                    .border_1()
                    .border_color(if !in_module_mode {
                        palette.thumb_rim()
                    } else {
                        rgba(0x00000000)
                    })
                    .text_xs()
                    .when(!in_module_mode, |d| {
                        d.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(in_module_mode, |d| d.text_color(palette.ink().opacity(0.68)))
                    .child("整体编辑")
                    .on_click(cx.listener(|this, _, w, cx| {
                        if !this.module.is_empty() || !this.choices.is_empty() {
                            this.switch_module("", w, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("editor-mode-module")
                    .cursor_pointer()
                    .px(px(10.))
                    .py(px(2.))
                    .rounded_full()
                    .border_1()
                    .border_color(if in_module_mode {
                        palette.thumb_rim()
                    } else {
                        rgba(0x00000000)
                    })
                    .text_xs()
                    .when(in_module_mode, |d| {
                        d.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(!in_module_mode, |d| d.text_color(palette.ink().opacity(0.68)))
                    .child("分模块编辑")
                    .on_click(cx.listener(|this, _, w, cx| {
                        if this.busy || !this.module.is_empty() {
                            return;
                        }
                        let full = this.full_text(cx);
                        match singboard_core::editor::modules(&full) {
                            Ok(list) => {
                                if let Some((first, range)) = list.first().cloned() {
                                    this.choices = list;
                                    this.select_module(first, range, w, cx);
                                }
                            }
                            Err(e) => {
                                this.status = e;
                                cx.notify();
                            }
                        }
                    })),
            );

        let mut mode_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(mode_tabs);
        if in_module_mode && !self.choices.is_empty() {
            let mut pills = div().flex().flex_wrap().items_center().gap_1();
            for (i, (name, _)) in self.choices.clone().into_iter().enumerate() {
                let active = name == self.module;
                pills = pills.child(
                    div()
                        .id(("module", i))
                        .cursor_pointer()
                        .px(px(10.))
                        .py(px(2.))
                        .rounded_full()
                        .text_xs()
                        .when(active, |d| {
                            d.bg(palette.primary_soft())
                                .text_color(palette.accent())
                                .font_weight(FontWeight::MEDIUM)
                        })
                        .when(!active, |d| {
                            d.bg(palette.fill())
                                .hover(|s| s.bg(palette.fill_strong()))
                                .text_color(palette.muted())
                        })
                        .child(name.clone())
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.switch_module(&name, w, cx);
                        })),
                );
            }
            mode_row = mode_row.child(pills);
        }
        mode_row = mode_row
            .child(div().flex_1())
            .when(!self.status.is_empty(), |row| {
                row.child(
                    div()
                        .max_w(px(360.))
                        .truncate()
                        .text_xs()
                        .text_color(palette.muted())
                        .child(self.status.clone()),
                )
            })
            .when(self.dirty, |row| {
                row.child(
                    div()
                        .min_h(crate::theme::text_px(20.))
                        .px_2()
                        .rounded_full()
                        .bg(palette.fill())
                        .flex()
                        .items_center()
                        .text_size(crate::theme::text_px(11.))
                        .text_color(palette.muted())
                        .child("未保存"),
                )
            });

        let editor_card = div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .rounded(px(18.))
            .overflow_hidden()
            .bg(palette.card_tinted(true, tint))
            .border_1()
            .border_color(palette.rim())
            .shadow(crate::glass::card_shadows(palette.0))
            .child(crate::glass::card_layer(
                palette.card_tinted(true, tint),
                px(17.),
                0.60,
            ))
            .child(crate::glass::grain_layer(px(17.)))
            .child(
                div()
                    .relative()
                    .size_full()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(bar)
                            .child(mode_row),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h(px(160.))
                            .w_full()
                            .rounded(px(12.))
                            .overflow_hidden()
                            .bg(if palette.0 {
                                rgba(0x1c1c2048)
                            } else {
                                rgba(0xffffff42)
                            })
                            .border_1()
                            .border_color(palette.control_rim())
                            .child(
                                CodeEditor::new(&self.input)
                                    .h_full()
                                    .appearance(false)
                                    .bordered(false)
                                    .disabled(self.busy),
                            ),
                    ),
            );

        div()
            .key_context("ConfigEditor")
            .on_action(cx.listener(|this, _: &SaveConfig, _, cx| this.check(true, cx)))
            .on_action(cx.listener(|this, _: &ToggleComment, w, cx| this.toggle_comment(w, cx)))
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .when(self.inline.is_none(), |d| d.p_4().bg(palette.page()))
            .text_color(palette.ink())
            .when_some(self.inline.as_ref(), |d, inline| {
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("return-list")
                                .size(px(28.))
                                .flex_none()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|s| s.bg(palette.fill()))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    svg()
                                        .path("back")
                                        .size(px(16.))
                                        .text_color(crate::glass::backdrop_ink(palette, cx)),
                                )
                                .on_click(cx.listener(|v, _, window, cx| {
                                    v.request(Pending::Close, window, cx)
                                })),
                        )
                        .child(
                            crate::glass::backdrop_heading(inline.name.clone(), palette, cx)
                                .text_size(crate::theme::text_px(22.))
                                .line_height(crate::theme::text_px(28.))
                                .font_weight(FontWeight::BOLD),
                        )
                        .child(
                            crate::glass::backdrop_pill(
                                if inline.remote { "远程副本" } else { "本地文件" },
                                true,
                                palette,
                                cx,
                            )
                            .flex_none()
                            .text_xs(),
                        ),
                )
                .when(inline.remote, |d| {
                    d.child(
                        crate::glass::backdrop_pill(
                            "下次更新（手动或自动）会用订阅内容覆盖这里的改动。",
                            true,
                            palette,
                            cx,
                        )
                        .flex_none()
                        .text_xs(),
                    )
                })
            })
            .when(self.inline.is_none(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .text_sm()
                        .truncate()
                        .text_color(palette.muted())
                        .child(self.doc.path.display().to_string()),
                )
            })
            .child(editor_card)
    }
}

#[cfg(test)]
mod tests {
    use super::{Core, Document, files, validate};
    use std::path::Path;

    #[test]
    fn saves_preserve_comments_encoding_and_external_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = "\u{feff}{\r\n  // 注释\r\n  \"log\": {},\r\n}\r\n";
        std::fs::write(&path, original).unwrap();
        let doc = Document::open(path.clone()).unwrap();
        assert_eq!(doc.encode(&doc.text), original);
        let next = doc.encode(&doc.text.replace("注释", "新注释"));
        doc.save(&next).unwrap();
        assert_eq!(
            std::fs::read_to_string(files::config_backup_path(&path)).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original.replace("注释", "新注释")
        );
        assert!(doc.save("{}").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), next);
    }

    #[test]
    fn validates_jsonc_without_accepting_json5_or_empty_documents() {
        singboard_core::runtime().block_on(async {
            let path = Path::new("config.json");
            assert!(
                validate(path, "{/* 注释 */\"url\":\"https://example.org\",}", None)
                    .await
                    .is_ok()
            );
            for content in ["", "null", "[]", "{a:1}", "{\"a\":1 \"b\":2}", "{\"v\":+1}"] {
                assert!(validate(path, content, None).await.is_err(), "{content}");
            }
        });
    }

    #[test]
    fn failed_core_check_leaves_no_validation_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let core = Core {
            executable: dir.path().join("missing.exe"),
            working_dir: dir.path().into(),
        };
        assert!(
            singboard_core::runtime()
                .block_on(validate(&path, "{}", Some(&core)))
                .is_err()
        );
        assert_eq!(dir.path().read_dir().unwrap().count(), 0);
    }
}
