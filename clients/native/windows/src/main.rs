#![windows_subsystem = "windows"]

mod chrome;
mod commands;
pub(crate) use singboard_stream::{settings, tools};
#[path = "../../stream/src/core_actions.rs"]
mod core_actions;
mod editor;
mod glass;
mod maintenance;
mod search;
mod service;
mod setup_wizard;
mod theme;
mod tray_menu;
mod wallpaper_editor;
mod window_state;
mod workspace;

use glass::Wallpaper;
use gpui::{prelude::*, *};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use singboard_stream::{Feed, Subscription};
use std::{path::Path, sync::Arc, time::Duration};
use tray_icon::{TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, SW_HIDE, SW_RESTORE, ShowWindow};
use workspace::{Page, Workspace};

struct Overview {
    title_opacity: f32,
    title_tick: std::time::Instant,
    workspace: Entity<Workspace>,
    wizard: Entity<setup_wizard::SetupWizard>,
    brand: Arc<gpui::Image>,
    wallpaper: Wallpaper,
    background: Arc<RenderImage>,
    material_busy: bool,
    stream: Option<Subscription>,
}

fn should_connect(prefs: &settings::Settings) -> bool {
    prefs.setup_completed
        || std::env::var_os("SINGBOARD_CLASH_URL").is_some()
        || (!prefs.singbox_path.trim().is_empty()
            && !prefs.working_dir.trim().is_empty()
            && prefs.active().is_some())
}

fn image_layer(image: Arc<RenderImage>, fit: ObjectFit) -> Img {
    img(image)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .object_fit(fit)
}

impl Render for Overview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::Palette::get(cx);
        if self.wallpaper.set_dark(palette.0) {
            let old = std::mem::replace(&mut self.background, self.wallpaper.background());
            cx.drop_image(old, Some(window));
        }
        self.update_card_backdrop(window, cx);
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .font_family(theme::system_ui_font())
            .text_color(palette.ink())
            .bg(palette.page())
            .child(image_layer(self.background.clone(), ObjectFit::Cover))
            .child(
                div()
                    .absolute()
                    .left(px(216.))
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .child(self.workspace.clone()),
            )
            .child(self.navigation(window, cx))
            .children(self.workspace.update(cx, |state, cx| state.page_overlay(window, cx)))
            .children(self.workspace.update(cx, |state, cx| match state.page {
                Page::Config => state.render_config_dialog(cx),
                Page::Settings => state.render_settings_dialog(cx),
                _ => None,
            }))
            .child(self.wizard.clone())
            .when(!self.wizard.read(cx).visible, |root| {
                root.children(self.workspace.update(cx, |state, cx| state.panel_update_overlay(window, cx)))
            })
            .children(self.workspace.read(cx).toasts.clone())
    }
}

fn close_window(window: &Window, cx: &mut App) {
    let _ = window_state::save(window.window_bounds());
    if maintenance::preferences()
        .ok()
        .and_then(|p| p.close_to_tray)
        .unwrap_or(true)
    {
        visibility(window, false)
    } else {
        editor::quit(cx)
    }
}

fn visibility(window: &Window, show: bool) {
    if let Ok(handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            unsafe {
                ShowWindow(
                    handle.hwnd.get() as _,
                    if show { SW_RESTORE } else { SW_HIDE },
                );
            }
        }
    }
    if show {
        window.activate_window();
    }
}

fn endpoint(prefs: &settings::Settings) -> (String, String) {
    let api = prefs.active();
    match std::env::var("SINGBOARD_CLASH_URL") {
        Ok(url) => (
            url,
            std::env::var("SINGBOARD_CLASH_SECRET").unwrap_or_default(),
        ),
        Err(_) => (
            api.map_or("http://127.0.0.1:9090", |p| p.url.as_str())
                .into(),
            std::env::var("SINGBOARD_CLASH_SECRET")
                .unwrap_or_else(|_| api.map(|p| p.secret.clone()).unwrap_or_default()),
        ),
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|v| v == maintenance::APPLY) {
        if !(args.len() == 4 || (args.len() == 6 && args[4] == "--wallpaper")) {
            return Err("无效的更新参数".into());
        }
        return maintenance::apply(
            Path::new(&args[1]),
            args[2]
                .to_str()
                .and_then(|v| v.parse().ok())
                .ok_or("进程标识无效")?,
            args[3].to_str().ok_or("校验值无效")?,
            &args[4..],
        );
    }
    if args
        .first()
        .is_some_and(|v| v == service::admin::ADMIN_FLAG)
    {
        let pipe = args.get(1).and_then(|a| a.to_str()).ok_or("缺少管理端点")?;
        let pid = args
            .get(2)
            .and_then(|a| a.to_str())
            .and_then(|v| v.parse().ok())
            .ok_or("缺少管理进程标识")?;
        return service::run_child(pipe, pid);
    }
    if args.first().is_some_and(|v| v == "--install-service") {
        if args.len() != 3 {
            return Err("用法：--install-service <核心路径> <配置路径>".into());
        }
        return service::install(Path::new(&args[1]), Path::new(&args[2]));
    }
    let hidden = args.iter().any(|a| a == "--hidden");
    if let Some(digest) = args
        .iter()
        .position(|a| a == maintenance::CLEANUP)
        .and_then(|i| args.get(i + 1))
        .and_then(|a| a.to_str())
    {
        maintenance::finish(digest);
    }
    let cli_wallpaper = args
        .iter()
        .position(|a| a == "--wallpaper")
        .map(|i| {
            args.get(i + 1)
                .map(std::path::PathBuf::from)
                .ok_or("--wallpaper 缺少文件路径")
        })
        .transpose()?;
    let wallpaper_file = cli_wallpaper.or_else(wallpaper_editor::saved_path);
    settings::repair_auto_launch();
    let Some(instance) = window_state::instance(hidden)? else {
        return Ok(());
    };
    let mut wallpaper = Wallpaper::load(wallpaper_file.as_deref())?;
    let prefs = maintenance::preferences()?;
    let (base, secret) = endpoint(&prefs);
    let initial_connect = should_connect(&prefs);
    let stream = if initial_connect {
        Some(Subscription::start(&base, &secret, Feed::Traffic)?)
    } else {
        None
    };
    let start = match prefs.start_page.trim_matches('/') {
        "overview" | "home" => Page::Overview,
        "connections" => Page::Connections,
        "rules" => Page::Rules,
        "logs" => Page::Logs,
        "config" => Page::Config,
        "settings" => Page::Settings,
        _ => Page::Proxies,
    };
    let initial_appearance = theme::Appearance::load();
    if initial_appearance.mode == theme::Mode::Dark {
        wallpaper.set_dark(true);
    }
    let mut workspace = Workspace::new(&base, &secret, &prefs, initial_connect)?;
    workspace.custom_wallpaper = wallpaper.custom;
    workspace.glass_tint = prefs.glass_tint.unwrap_or(1.);
    if !hidden {
        workspace.prewarm(start);
        wallpaper.prewarm_material(workspace.glass_tint, size(px(1100.), px(720.)));
    }
    gpui_platform::application()
        .with_assets(chrome::Assets)
        .run(move |cx: &mut App| {
            let _ = cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(
                include_bytes!("../assets/TwemojiMozilla-flags.ttf"),
            )]);
            gpui_component::init(cx);
            editor::init(cx);
            wallpaper_editor::init(cx);
            gpui_component::set_locale("zh-CN");
            cx.set_global(initial_appearance);
            let workspace = cx.new(|cx| {
                let mut workspace = workspace;
                workspace.open(start, cx);
                if prefs.panel_auto_check_update.unwrap_or(true) {
                    workspace.check_update(cx);
                }
                workspace
            });
            tray_menu::init(cx);
            let icon =
                image::load_from_memory(include_bytes!("../icons/32x32.png"))
                    .unwrap()
                    .into_rgba8();
            let (w, h) = icon.dimensions();
            let tray = TrayIconBuilder::new()
                .with_tooltip("Singboard · GPUI Fast")
                .with_icon(tray_icon::Icon::from_rgba(icon.into_raw(), w, h).unwrap())
                .build()
                .expect("创建托盘");
            let show_wizard = !hidden && !initial_connect;
            let wizard_prefs = prefs.clone();
            let bounds = window_state::restore(cx);
            let mut overview_entity = None;
            let handle = cx
                .open_window(
                    WindowOptions {
                        show: !hidden,
                        focus: !hidden,
                        window_bounds: Some(bounds),
                        window_min_size: Some(size(px(800.), px(500.))),
                        titlebar: Some(TitlebarOptions {
                            title: Some("singboard".into()),
                            appears_transparent: true,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    |window, cx| {
                        window.on_window_should_close(cx, |window, cx| {
                            close_window(window, cx);
                            false
                        });
                        let custom = wallpaper.custom;
                        theme::Appearance::apply(window, custom, cx);
                        let wizard = cx.new(|cx| {
                            setup_wizard::SetupWizard::new(
                                &wizard_prefs,
                                workspace.clone(),
                                show_wizard,
                                window,
                                cx,
                            )
                        });
                        let view = cx.new(|cx| {
                            cx.observe_window_appearance(window, |view: &mut Overview, window, cx| {
                                theme::Appearance::apply(window, view.wallpaper.custom, cx);
                            })
                            .detach();
                            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
                            cx.observe(&wizard, |_, _, cx| cx.notify()).detach();
                            Overview {
                                title_opacity: 0.,
                                title_tick: std::time::Instant::now(),
                                workspace,
                                wizard,
                                brand: Arc::new(gpui::Image::from_bytes(
                                    ImageFormat::Png,
                                    include_bytes!("../../android/assets/favicon.png").to_vec(),
                                )),
                                background: wallpaper.background(),
                                material_busy: false,
                                wallpaper,
                                stream,
                            }
                        });
                        overview_entity = Some(view.clone());
                        cx.new(|cx| gpui_component::Root::new(view, window, cx))
                    },
                )
                .expect("创建 GPUI 窗口");
            let overview = overview_entity.expect("主视图已创建");
            cx.spawn(async move |cx| {
                let _tray = tray;
                let mut revision = settings::Settings::revision();
                let mut wp_rev = wallpaper_editor::revision();
                let mut current = (base, secret);
                let mut connected = initial_connect;
                let mut geometry = bounds;
                let mut observed = bounds;
                let mut moved = std::time::Instant::now();
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(120))
                        .await;
                    let mut show = instance.requested();
                    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                        if let TrayIconEvent::Click { button, button_state: tray_icon::MouseButtonState::Up, position, .. } = event {
                            match button {
                                tray_icon::MouseButton::Left => show = true,
                                tray_icon::MouseButton::Right => {
                                    let _ = cx.update(|cx| {
                                        let ws = overview.read(cx).workspace.clone();
                                        tray_menu::open(position.x as i32, position.y as i32, handle.into(), ws, cx);
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    if handle
                        .update(cx, |_, window, cx| {
                            overview.update(cx, |panel, cx| {
                                let now = window.window_bounds();
                                if now != observed {
                                    observed = now;
                                    moved = std::time::Instant::now();
                                }
                                if geometry != observed
                                    && moved.elapsed() >= Duration::from_millis(450)
                                    && window_state::save(observed).is_ok()
                                {
                                    geometry = observed;
                                }
                                let saved = settings::Settings::revision();
                                if revision != saved
                                    && let Ok(prefs) = maintenance::preferences()
                                {
                                    let next = endpoint(&prefs);
                                    let want_connect = should_connect(&prefs);
                                    panel.workspace.update(cx, |state, cx| {
                                        if want_connect && (!connected || current != next) {
                                            if let (Ok(mut new), Ok(stream)) = (
                                                Workspace::new(&next.0, &next.1, &prefs, true),
                                                Subscription::start(&next.0, &next.1, Feed::Traffic),
                                            ) {
                                                let page = state.page;
                                                new.custom_wallpaper = state.custom_wallpaper;
                                                new.glass_tint = prefs.glass_tint.unwrap_or(1.);
                                                new.inherit_view(state);
                                                *state = new;
                                                state.open(page, cx);
                                                panel.stream = Some(stream);
                                                current = next;
                                                connected = true;
                                            }
                                        } else {
                                            state.preferences(&prefs, cx);
                                        }
                                    });
                                    let appearance = theme::Appearance::load();
                                    if appearance.mode != cx.global::<theme::Appearance>().mode {
                                        cx.global_mut::<theme::Appearance>().mode = appearance.mode;
                                        theme::Appearance::apply(window, panel.wallpaper.custom, cx);
                                    }
                                    let tint_val = prefs.glass_tint.unwrap_or(1.);
                                    panel.workspace.update(cx, |state, cx| {
                                        state.glass_tint = tint_val;
                                        cx.notify();
                                    });
                                    revision = saved;
                                    cx.notify();
                                }
                                let wp_saved = wallpaper_editor::revision();
                                if wp_rev != wp_saved {
                                    wp_rev = wp_saved;
                                    if let Ok(mut next_wp) =
                                        Wallpaper::load(wallpaper_editor::saved_path().as_deref())
                                    {
                                        let dark = cx.global::<theme::Appearance>().dark;
                                        next_wp.set_dark(dark);
                                        let custom = next_wp.custom;
                                        let old_bg = std::mem::replace(
                                            &mut panel.background,
                                            next_wp.background(),
                                        );
                                        cx.drop_image(old_bg, Some(window));
                                        panel.wallpaper = next_wp;
                                        panel.workspace.update(cx, |state, cx| {
                                            state.custom_wallpaper = custom;
                                            cx.notify();
                                        });
                                        theme::Appearance::apply(window, custom, cx);
                                        cx.notify();
                                    }
                                }
                                if let Some(stream) = &panel.stream {
                                    let batch = stream.drain();
                                    panel.workspace.update(cx, |state, cx| {
                                        state.push_traffic(batch.status, batch.failures, batch.has_frame, batch.traffic, cx);
                                        state.tick(cx);
                                    });
                                } else {
                                    panel.workspace.update(cx, |state, cx| {
                                        state.tick(cx);
                                    });
                                }
                                if show {
                                    visibility(window, true);
                                }
                            });
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            })
            .detach();
        });
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        let message = format!("{error}\0").encode_utf16().collect::<Vec<_>>();
        let title = "Singboard\0".encode_utf16().collect::<Vec<_>>();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), message.as_ptr(), title.as_ptr(), 0x10);
        }
        std::process::exit(1);
    }
}
