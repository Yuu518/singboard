use crate::{glass::render_image, service, theme::Palette};
use gpui::{prelude::*, *};
use gpui_component::WindowExt;
use image::{DynamicImage, RgbaImage, imageops};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static WALLPAPER_REVISION: AtomicU64 = AtomicU64::new(0);

pub fn revision() -> u64 {
    WALLPAPER_REVISION.load(Ordering::Acquire)
}

pub fn notify_changed() {
    WALLPAPER_REVISION.fetch_add(1, Ordering::Release);
}

pub fn saved_path() -> Option<PathBuf> {
    let path = singboard_core::settings::wallpaper_path(&service::data_dir().ok()?);
    path.is_file().then_some(path)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AspectPreset {
    Free,
    Window,
}

const PRESETS: [(AspectPreset, &str); 2] = [
    (AspectPreset::Free, "自由"),
    (AspectPreset::Window, "适配窗口"),
];

#[derive(Clone, Copy)]
struct Transform {
    rotation: u8,
    flip_h: bool,
    flip_v: bool,
    aspect: AspectPreset,
    crop: Option<Crop>,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            rotation: 0,
            flip_h: false,
            flip_v: false,
            aspect: AspectPreset::Free,
            crop: None,
        }
    }
}

fn orient(source: &RgbaImage, tf: Transform) -> RgbaImage {
    let mut img = DynamicImage::ImageRgba8(source.clone());
    img = match tf.rotation % 4 {
        1 => img.rotate90(),
        2 => img.rotate180(),
        3 => img.rotate270(),
        _ => img,
    };
    if tf.flip_h {
        img = img.fliph();
    }
    if tf.flip_v {
        img = img.flipv();
    }
    img.to_rgba8()
}

#[derive(Clone, Copy)]
struct Crop {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

const HANDLES: [(i8, i8); 8] = [
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
    (0, -1),
    (-1, 0),
    (1, 0),
    (0, 1),
];

impl Crop {
    fn selection(tf: Transform, w: f32, h: f32, win_ratio: f32) -> Self {
        if let Some(crop) = tf.crop {
            return crop;
        }
        let aspect = match tf.aspect {
            AspectPreset::Free => w / h,
            AspectPreset::Window => win_ratio,
        } / (w / h);
        let width = aspect.min(1.);
        let height = (1. / aspect).min(1.);
        Self {
            left: (1. - width) * 0.5,
            top: (1. - height) * 0.5,
            width,
            height,
        }
    }

    fn drag(
        self,
        handle: (i8, i8),
        dx: f32,
        dy: f32,
        min_w: f32,
        min_h: f32,
        locked: bool,
    ) -> Self {
        let (hx, hy) = handle;
        if handle == (0, 0) {
            return Self {
                left: (self.left + dx).clamp(0., (1. - self.width).max(0.)),
                top: (self.top + dy).clamp(0., (1. - self.height).max(0.)),
                ..self
            };
        }
        let right = self.left + self.width;
        let bottom = self.top + self.height;
        let min_w = min_w.min(self.width);
        let min_h = min_h.min(self.height);
        if locked {
            let ratio = self.width / self.height;
            let max_w = (if hx > 0 { 1. - self.left } else { right })
                .min((if hy > 0 { 1. - self.top } else { bottom }) * ratio);
            let width = (self.width + hx as f32 * dx)
                .max((self.height + hy as f32 * dy) * ratio)
                .clamp(min_w.max(min_h * ratio).min(max_w), max_w);
            let height = width / ratio;
            return Self {
                left: if hx > 0 { self.left } else { right - width },
                top: if hy > 0 { self.top } else { bottom - height },
                width,
                height,
            };
        }
        let left = if hx < 0 {
            (self.left + dx).clamp(0., (right - min_w).max(0.))
        } else {
            self.left
        };
        let top = if hy < 0 {
            (self.top + dy).clamp(0., (bottom - min_h).max(0.))
        } else {
            self.top
        };
        let right = if hx > 0 {
            (right + dx).clamp((self.left + min_w).min(1.), 1.)
        } else {
            right
        };
        let bottom = if hy > 0 {
            (bottom + dy).clamp((self.top + min_h).min(1.), 1.)
        } else {
            bottom
        };
        Self {
            left,
            top,
            width: right - left,
            height: bottom - top,
        }
    }
}

fn apply_transform(source: &RgbaImage, tf: Transform, win_ratio: f32) -> RgbaImage {
    let image = orient(source, tf);
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return image;
    }
    let crop = Crop::selection(tf, w as f32, h as f32, win_ratio);
    let x = (crop.left * w as f32).round().clamp(0., (w - 1) as f32) as u32;
    let y = (crop.top * h as f32).round().clamp(0., (h - 1) as f32) as u32;
    let width = (crop.width * w as f32).round().clamp(1., (w - x) as f32) as u32;
    let height = (crop.height * h as f32).round().clamp(1., (h - y) as f32) as u32;
    imageops::crop_imm(&image, x, y, width, height).to_image()
}

#[derive(Default)]
struct PickingWallpaper(bool);
impl Global for PickingWallpaper {}

pub fn init(cx: &mut App) {
    cx.set_global(PickingWallpaper::default());
}

pub fn open(win_ratio: f32, window: &mut Window, cx: &mut App) {
    if cx.global::<PickingWallpaper>().0 {
        return;
    }
    cx.global_mut::<PickingWallpaper>().0 = true;
    let handle = window.window_handle();
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("选择背景图片（PNG / JPG / WebP / BMP）".into()),
    });
    cx.spawn(async move |cx| {
        let answer = prompt.await;
        cx.update(|cx| cx.global_mut::<PickingWallpaper>().0 = false);
        let result = match answer {
            Ok(Ok(Some(paths))) => {
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                singboard_core::runtime()
                    .spawn_blocking(move || load_rgba(&path))
                    .await
                    .map_err(|err| err.to_string())
                    .and_then(|v| v)
            }
            Ok(Err(error)) => Err(error.to_string()),
            _ => return,
        };
        let _ = handle.update(cx, |_, window, cx| match result {
            Ok(source) => {
                let view = cx.new(|cx| {
                    cx.on_release(|this: &mut WallpaperEditor, cx| {
                        if let Some(old) = this.preview.take() {
                            cx.drop_image(old, None);
                        }
                    })
                    .detach();
                    let thumb = if source.width() > 960 || source.height() > 640 {
                        DynamicImage::ImageRgba8(source.clone())
                            .thumbnail(960, 640)
                            .to_rgba8()
                    } else {
                        source.clone()
                    };
                    let mut view = WallpaperEditor {
                        source,
                        thumb,
                        preview: None,
                        drag: None,
                        focus: cx.focus_handle(),
                        transform: Transform::default(),
                        win_ratio,
                        status: String::new(),
                        busy: false,
                    };
                    view.rebuild_preview(window, cx);
                    view
                });
                window.open_dialog(cx, move |dialog, window, cx| {
                    let busy = view.read(cx).busy;
                    let width = (window.viewport_size().width - px(32.)).min(px(820.));
                    dialog
                        .title("编辑背景")
                        .width(width)
                        .overlay_closable(false)
                        .keyboard(!busy)
                        .close_button(!busy)
                        .child(view.clone())
                });
            }
            Err(error) => window.open_dialog(cx, move |dialog, _, _| {
                dialog.title("读取背景图片失败").child(error.clone())
            }),
        });
    })
    .detach();
}

fn load_rgba(path: &Path) -> Result<RgbaImage, String> {
    let img = image::open(path).map_err(|e| format!("读取图片失败：{e}"))?;
    Ok(img.to_rgba8())
}

struct WallpaperEditor {
    source: RgbaImage,
    thumb: RgbaImage,
    preview: Option<Arc<RenderImage>>,
    drag: Option<(Crop, (i8, i8), Point<Pixels>)>,
    focus: FocusHandle,
    transform: Transform,
    win_ratio: f32,
    status: String,
    busy: bool,
}

fn stage_frame(area: Bounds<Pixels>, w: u32, h: u32) -> Bounds<Pixels> {
    let w = w.max(1) as f32;
    let h = h.max(1) as f32;
    let scale = ((f32::from(area.size.width) - 28.).max(1.) / w)
        .min((f32::from(area.size.height) - 28.).max(1.) / h);
    let width = px(w * scale);
    let height = px(h * scale);
    Bounds::new(
        point(
            area.origin.x + (area.size.width - width) * 0.5,
            area.origin.y + (area.size.height - height) * 0.5,
        ),
        size(width, height),
    )
}

impl WallpaperEditor {
    fn rotate(&mut self, clockwise: bool, window: &mut Window, cx: &mut Context<Self>) {
        let tf = &mut self.transform;
        tf.rotation = (tf.rotation + if clockwise { 1 } else { 3 }) % 4;
        std::mem::swap(&mut tf.flip_h, &mut tf.flip_v);
        tf.crop = tf
            .crop
            .filter(|_| tf.aspect == AspectPreset::Free)
            .map(|crop| Crop {
                left: if clockwise {
                    1. - crop.top - crop.height
                } else {
                    crop.top
                },
                top: if clockwise {
                    crop.left
                } else {
                    1. - crop.left - crop.width
                },
                width: crop.height,
                height: crop.width,
            });
        self.rebuild_preview(window, cx);
    }

    fn flip(&mut self, horizontal: bool, window: &mut Window, cx: &mut Context<Self>) {
        if horizontal {
            self.transform.flip_h = !self.transform.flip_h;
        } else {
            self.transform.flip_v = !self.transform.flip_v;
        }
        if let Some(crop) = &mut self.transform.crop {
            if horizontal {
                crop.left = 1. - crop.left - crop.width;
            } else {
                crop.top = 1. - crop.top - crop.height;
            }
        }
        self.rebuild_preview(window, cx);
    }

    fn crop_stage(&self, preview: Arc<RenderImage>, cx: &Context<Self>) -> Stateful<Div> {
        let (mut w, mut h) = self.source.dimensions();
        if self.transform.rotation % 2 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        let crop = Crop::selection(self.transform, w as f32, h as f32, self.win_ratio);
        let locked = self.transform.aspect != AspectPreset::Free;
        let handles = &HANDLES[..if locked { 4 } else { 8 }];
        let bounds = std::rc::Rc::new(std::cell::Cell::new(None::<Bounds<Pixels>>));
        let painted = bounds.clone();
        let move_entity = cx.entity().downgrade();
        let up_entity = cx.entity().downgrade();
        div()
            .id("wallpaper-crop")
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .child(
                canvas(
                    move |area, _, _| {
                        let frame = stage_frame(area, w, h);
                        painted.set(Some(frame));
                        frame
                    },
                    move |_, frame, window, _| {
                        let _ = window.paint_image(
                            frame,
                            frame,
                            Corners::all(px(0.)),
                            preview,
                            0,
                            false,
                        );
                        let frame_w = frame.size.width;
                        let frame_h = frame.size.height;
                        let x = frame.origin.x + frame_w * crop.left;
                        let y = frame.origin.y + frame_h * crop.top;
                        let width = frame_w * crop.width;
                        let height = frame_h * crop.height;
                        let edge_x = x + width;
                        let edge_y = y + height;
                        for (left, top, w, h) in [
                            (frame.origin.x, frame.origin.y, frame_w, y - frame.origin.y),
                            (
                                frame.origin.x,
                                edge_y,
                                frame_w,
                                frame.origin.y + frame_h - edge_y,
                            ),
                            (frame.origin.x, y, x - frame.origin.x, height),
                            (edge_x, y, frame.origin.x + frame_w - edge_x, height),
                        ] {
                            if w > px(0.) && h > px(0.) {
                                window.paint_quad(fill(
                                    Bounds::new(point(left, top), size(w, h)),
                                    rgba(0x00000077),
                                ));
                            }
                        }
                        for part in [0., 1. / 3., 2. / 3., 1.] {
                            window.paint_quad(fill(
                                Bounds::new(point(x + width * part, y), size(px(1.), height)),
                                rgba(0xffffffb3),
                            ));
                            window.paint_quad(fill(
                                Bounds::new(point(x, y + height * part), size(width, px(1.))),
                                rgba(0xffffffb3),
                            ));
                        }
                        for &(hx, hy) in handles {
                            let center = point(
                                x + width * ((hx as f32 + 1.) * 0.5),
                                y + height * ((hy as f32 + 1.) * 0.5),
                            );
                            window.paint_quad(quad(
                                Bounds::new(center - point(px(4.), px(4.)), size(px(8.), px(8.))),
                                px(4.),
                                rgb(0xffffff),
                                px(1.),
                                rgba(0x00000055),
                                BorderStyle::default(),
                            ));
                        }
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let _ = move_entity.update(cx, |editor, cx| {
                                let Some((start, handle, origin)) = editor.drag else {
                                    return;
                                };
                                if event.pressed_button != Some(MouseButton::Left) {
                                    editor.drag = None;
                                    return;
                                }
                                let width = f32::from(frame.size.width).max(1.);
                                let height = f32::from(frame.size.height).max(1.);
                                editor.transform.crop = Some(start.drag(
                                    handle,
                                    f32::from(event.position.x - origin.x) / width,
                                    f32::from(event.position.y - origin.y) / height,
                                    64. / w as f32,
                                    64. / h as f32,
                                    locked,
                                ));
                                cx.stop_propagation();
                                cx.notify();
                            });
                        });
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                                return;
                            }
                            let _ = up_entity.update(cx, |editor, _| {
                                editor.drag = None;
                            });
                        });
                    },
                )
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |editor, event: &MouseDownEvent, window, cx| {
                    if editor.busy {
                        return;
                    }
                    let Some(frame) = bounds.get() else { return };
                    window.focus(&editor.focus, cx);
                    let rx =
                        f32::from(event.position.x - frame.origin.x) / f32::from(frame.size.width);
                    let ry =
                        f32::from(event.position.y - frame.origin.y) / f32::from(frame.size.height);
                    let mut handle = (rx >= crop.left
                        && rx <= crop.left + crop.width
                        && ry >= crop.top
                        && ry <= crop.top + crop.height)
                        .then_some((0, 0));
                    let mut nearest = if handle.is_some() {
                        (crop.width * f32::from(frame.size.width))
                            .min(crop.height * f32::from(frame.size.height))
                            .mul_add(0.35, 0.)
                            .clamp(5., 14.)
                    } else {
                        14.
                    };
                    for &(hx, hy) in handles {
                        let dx = (crop.left + crop.width * (hx as f32 + 1.) * 0.5 - rx)
                            * f32::from(frame.size.width);
                        let dy = (crop.top + crop.height * (hy as f32 + 1.) * 0.5 - ry)
                            * f32::from(frame.size.height);
                        let distance = dx.hypot(dy);
                        if distance < nearest {
                            nearest = distance;
                            handle = Some((hx, hy));
                        }
                    }
                    editor.drag = handle.map(|h| (crop, h, event.position));
                    if editor.drag.is_some() {
                        cx.stop_propagation();
                    }
                }),
            )
            .on_key_down(
                cx.listener(move |editor, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter" {
                        editor.save(window, cx);
                        return;
                    }
                    let (dx, dy) = match event.keystroke.key.as_str() {
                        "left" => (-1., 0.),
                        "right" => (1., 0.),
                        "up" => (0., -1.),
                        "down" => (0., 1.),
                        _ => return,
                    };
                    let handle = if event.keystroke.modifiers.shift {
                        (1, 1)
                    } else {
                        (0, 0)
                    };
                    editor.transform.crop = Some(crop.drag(
                        handle,
                        0.01 * dx,
                        0.01 * dy,
                        64. / w as f32,
                        64. / h as f32,
                        locked,
                    ));
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
    }

    fn rebuild_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = render_image(orient(&self.thumb, self.transform));
        if let Some(old) = self.preview.replace(next) {
            cx.drop_image(old, Some(window));
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = "正在处理并保存壁纸…".into();
        let source = self.source.clone();
        let tf = self.transform;
        let ratio = self.win_ratio;
        let handle = window.window_handle();
        let task = singboard_core::runtime().spawn_blocking(move || {
            let dir = service::data_dir()?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let edited = apply_transform(&source, tf, ratio);
            let image = DynamicImage::ImageRgba8(edited);
            let image = if image.width().max(image.height()) > 4096 {
                image.thumbnail(4096, 4096)
            } else {
                image
            };
            let mut bytes = Vec::new();
            image
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .map_err(|e| format!("编码 PNG 失败：{e}"))?;
            let target = singboard_core::settings::wallpaper_path(&dir);
            singboard_core::profiles::write_atomic(&target, &bytes)?;
            Ok::<_, String>(target)
        });
        cx.spawn(async move |weak, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = weak.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(_) => {
                        notify_changed();
                        this.status = "壁纸已应用".into();
                        let _ = handle.update(cx, |_, w, cx| w.close_dialog(cx));
                    }
                    Err(e) => this.status = e,
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn tool_btn(id: impl Into<ElementId>, label: &'static str, palette: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded(px(10.))
        .bg(palette.fill())
        .hover(|s| s.bg(palette.fill_strong()))
        .text_sm()
        .cursor_pointer()
        .flex()
        .items_center()
        .gap_1()
        .when_some(
            match label {
                "左转 90°" => Some("rotate-left"),
                "右转 90°" => Some("rotate-right"),
                "水平镜像" => Some("flip-x"),
                "垂直镜像" => Some("flip-y"),
                _ => None,
            },
            |d, icon| d.child(svg().path(icon).size(px(15.)).text_color(palette.ink())),
        )
        .child(label)
}

impl Render for WallpaperEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::get(cx);
        let mut preset_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .p(px(3.))
            .rounded_full()
            .bg(palette.segment_track())
            .border_1()
            .border_color(palette.control_rim());
        for (i, (preset, label)) in PRESETS.into_iter().enumerate() {
            let active = self.transform.aspect == preset;
            preset_row = preset_row.child(
                div()
                    .id(("aspect", i))
                    .px_3()
                    .py(px(3.))
                    .rounded_full()
                    .border_1()
                    .border_color(if active {
                        palette.thumb_rim()
                    } else {
                        rgba(0x00000000)
                    })
                    .text_xs()
                    .cursor_pointer()
                    .when(active, |el| {
                        el.bg(palette.thumb())
                            .shadow_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.ink())
                    })
                    .when(!active, |el| el.text_color(palette.ink().opacity(0.68)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.transform.aspect == preset { return; }
                        let (mut w, mut h) = this.source.dimensions();
                        if this.transform.rotation % 2 == 1 { std::mem::swap(&mut w, &mut h); }
                        let old = Crop::selection(this.transform, w as f32, h as f32, this.win_ratio);
                        this.transform.aspect = preset;
                        this.transform.crop = (preset == AspectPreset::Free).then_some(old);
                        cx.notify();
                    })),
            );
        }

        let controls = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(tool_btn("rot-l", "左转 90°", palette).on_click(cx.listener(
                |this, _, window, cx| {
                    this.rotate(false, window, cx);
                    cx.notify();
                },
            )))
            .child(tool_btn("rot-r", "右转 90°", palette).on_click(cx.listener(
                |this, _, window, cx| {
                    this.rotate(true, window, cx);
                    cx.notify();
                },
            )))
            .child(
                tool_btn("flip-h", "水平镜像", palette).on_click(cx.listener(|this, _, window, cx| {
                    this.flip(true, window, cx);
                    cx.notify();
                })),
            )
            .child(
                tool_btn("flip-v", "垂直镜像", palette).on_click(cx.listener(|this, _, window, cx| {
                    this.flip(false, window, cx);
                    cx.notify();
                })),
            )
            .child(tool_btn("reset-tf", "重置", palette).on_click(cx.listener(
                |this, _, window, cx| {
                    this.transform = Transform { aspect: this.transform.aspect, ..Default::default() };
                    this.rebuild_preview(window, cx);
                    cx.notify();
                },
            )));

        let footer = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(palette.muted())
                    .child(if self.status.is_empty() {
                        "方向键移动，Shift + 方向键调整大小，Enter 使用".to_owned()
                    } else {
                        self.status.clone()
                    }),
            )
            .child(
                tool_btn("cancel-wallpaper", "取消", palette).on_click(cx.listener(
                    |this, _, window, cx| {
                        if !this.busy {
                            window.close_dialog(cx);
                        }
                    },
                )),
            )
            .child(
                tool_btn(
                    "apply-wp",
                    if self.busy {
                        "保存中…"
                    } else {
                        "使用这张背景"
                    },
                    palette,
                )
                .bg(palette.primary_fill())
                .text_color(rgb(0xffffff))
                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            );
        div()
            .w_full()
            .font_family(crate::theme::system_ui_font())
            .flex()
            .flex_col()
            .gap_3()
            .text_color(palette.ink())
            .child(
                div()
                    .h((window.viewport_size().height * 0.58).min(px(460.)))
                    .min_h(px(120.))
                    .w_full()
                    .rounded(px(14.))
                    .overflow_hidden()
                    .bg(palette.fill())
                    .when_some(self.preview.clone(), |el, preview| {
                        el.child(self.crop_stage(preview, cx))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(controls)
                    .child(preset_row),
            )
            .child(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::{AspectPreset, Crop, Transform, apply_transform, stage_frame};
    use gpui::{Bounds, point, px, size};
    use image::RgbaImage;

    #[test]
    fn stage_frame_fits_and_centers_all_aspect_ratios() {
        let area = Bounds::new(point(px(20.), px(10.)), size(px(628.), px(368.)));
        let tall = stage_frame(area, 1920, 1386);
        assert!((f32::from(tall.size.height) - 340.).abs() < 0.01);
        assert!(f32::from(tall.size.width) < 600.);
        assert!((f32::from(tall.origin.y) - 24.).abs() < 0.01);
        assert!(
            (f32::from(tall.origin.x + tall.size.width * 0.5)
                - f32::from(area.origin.x + area.size.width * 0.5))
            .abs()
                < 0.01
        );

        let wide = stage_frame(area, 2560, 1080);
        assert!((f32::from(wide.size.width) - 600.).abs() < 0.01);
        assert!(f32::from(wide.size.height) < 340.);
        assert!((f32::from(wide.origin.x) - 34.).abs() < 0.01);
    }

    #[test]
    fn free_and_locked_handles_keep_the_opposite_edge_inside_the_image() {
        let crop = Crop { left: 0.25, top: 0.25, width: 0.5, height: 0.5 };
        let free = crop.drag((-1, 0), -0.5, 0.8, 0.05, 0.05, false);
        assert_eq!((free.left, free.width, free.top, free.height), (0., 0.75, 0.25, 0.5));
        let fixed = crop.drag((1, 1), 0.75, 0.01, 0.05, 0.05, true);
        assert_eq!((fixed.width, fixed.height, fixed.left, fixed.top), (0.75, 0.75, 0.25, 0.25));
        let moved = crop.drag((0, 0), 8., -2., 0.05, 0.05, false);
        assert_eq!((moved.left, moved.top, moved.width, moved.height), (0.5, 0., 0.5, 0.5));
    }

    #[test]
    fn transform_rotates_and_crops_aspect_ratios() {
        let src = RgbaImage::from_pixel(200, 100, image::Rgba([10, 20, 30, 255]));
        let rotated = apply_transform(
            &src,
            Transform {
                rotation: 1,
                ..Default::default()
            },
            1.6,
        );
        assert_eq!(rotated.dimensions(), (100, 200));

        let wide = apply_transform(
            &src,
            Transform {
                aspect: AspectPreset::Window,
                crop: Some(super::Crop {
                    left: 0.2,
                    top: 0.25,
                    width: 1. / 3.,
                    height: 0.5,
                }),
                ..Default::default()
            },
            1.6,
        );
        assert_eq!(wide.dimensions(), (67, 50));
    }
}
