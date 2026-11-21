use gpui::{prelude::*, *};
use image::{Frame, Rgba, RgbaImage};
use std::f32::consts::PI;
use std::path::Path;
use std::sync::{Arc, OnceLock};

struct Bloom {
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    cos_a: f32,
    sin_a: f32,
    rgb: [f32; 3],
    strength: f32,
}

impl Bloom {
    fn new(cx: f32, cy: f32, rx: f32, ry: f32, angle: f32, rgb: [u8; 3], strength: f32) -> Self {
        Self {
            cx,
            cy,
            rx,
            ry,
            cos_a: angle.cos(),
            sin_a: angle.sin(),
            rgb: [f32::from(rgb[0]), f32::from(rgb[1]), f32::from(rgb[2])],
            strength,
        }
    }

    #[inline]
    fn weight(&self, u: f32, v: f32) -> f32 {
        let dx = u - self.cx;
        let dy = v - self.cy;
        let rx = (dx * self.cos_a + dy * self.sin_a) / self.rx;
        let ry = (-dx * self.sin_a + dy * self.cos_a) / self.ry;
        let d2 = rx * rx + ry * ry;
        if d2 >= 1.0 {
            0.0
        } else {
            let t = 1.0 - d2;
            t * t * t * self.strength
        }
    }
}

#[inline]
fn hash_dither(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    let a = (h & 0xFF) as f32 / 255.0;
    let b = ((h >> 8) & 0xFF) as f32 / 255.0;
    (a - b) * 1.35
}

#[inline]
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn default_backdrop(dark: bool) -> RgbaImage {
    const W: u32 = 960;
    const H: u32 = 600;

    let (c_tl, c_tr, c_bl, c_br, blooms, ribbon_core, ribbon_skirt, core_k, skirt_k) = if dark {
        (
            [8.0f32, 13.0, 26.0],
            [14.0f32, 16.0, 32.0],
            [9.0f32, 18.0, 30.0],
            [17.0f32, 13.0, 29.0],
            [
                Bloom::new(0.12, 0.16, 0.66, 0.50, -0.36, [18, 118, 255], 0.36),
                Bloom::new(0.86, 0.18, 0.60, 0.48, 0.42, [104, 88, 246], 0.30),
                Bloom::new(0.20, 0.88, 0.68, 0.46, 0.24, [16, 178, 214], 0.24),
                Bloom::new(0.82, 0.80, 0.58, 0.46, -0.32, [172, 76, 218], 0.20),
            ],
            [96.0f32, 184.0, 255.0],
            [126.0f32, 108.0, 250.0],
            0.14f32,
            0.09f32,
        )
    } else {
        (
            [231.0f32, 240.0, 253.0],
            [238.0f32, 237.0, 252.0],
            [230.0f32, 245.0, 250.0],
            [243.0f32, 237.0, 249.0],
            [
                Bloom::new(0.13, 0.16, 0.65, 0.48, -0.35, [86, 172, 255], 0.38),
                Bloom::new(0.86, 0.18, 0.60, 0.48, 0.42, [142, 130, 255], 0.30),
                Bloom::new(0.22, 0.88, 0.68, 0.46, 0.25, [72, 214, 238], 0.27),
                Bloom::new(0.80, 0.78, 0.58, 0.45, -0.30, [238, 156, 224], 0.22),
            ],
            [255.0f32, 255.0, 255.0],
            [164.0f32, 220.0, 255.0],
            0.42f32,
            0.16f32,
        )
    };

    let inv_w = 1.0 / (W - 1) as f32;
    let inv_h = 1.0 / (H - 1) as f32;

    let mut wave = Vec::with_capacity(W as usize);
    for x in 0..W {
        let u = x as f32 * inv_w;
        let crest = 0.52 - 0.18 * (u - 0.5)
            + 0.085 * (u * PI * 2.1 - 0.4).sin()
            + 0.035 * (u * PI * 3.7 + 0.8).cos();
        let crest2 = 0.30 + 0.14 * (u - 0.5) + 0.065 * (u * PI * 1.7 + 1.1).cos();
        wave.push((crest, crest2));
    }

    RgbaImage::from_fn(W, H, |x, y| {
        let u = x as f32 * inv_w;
        let v = y as f32 * inv_h;
        let us = u * u * (3.0 - 2.0 * u);
        let vs = v * v * (3.0 - 2.0 * v);

        let mut rgb = [0.0f32; 3];
        for i in 0..3 {
            let top = mix(c_tl[i], c_tr[i], us);
            let bot = mix(c_bl[i], c_br[i], us);
            rgb[i] = mix(top, bot, vs);
        }

        for bloom in &blooms {
            let w = bloom.weight(u, v);
            if w > 0.0 {
                for i in 0..3 {
                    rgb[i] = mix(rgb[i], bloom.rgb[i], w);
                }
            }
        }

        let (crest, crest2) = wave[x as usize];
        let dv1 = (v - crest) / 0.095;
        let core = (-dv1 * dv1).exp() * core_k;
        let dv2 = (v - crest2) / 0.19;
        let skirt = (-dv2 * dv2).exp() * skirt_k;

        for i in 0..3 {
            rgb[i] = mix(rgb[i], ribbon_skirt[i], skirt);
            rgb[i] = mix(rgb[i], ribbon_core[i], core);
        }

        let dither = hash_dither(x, y);
        Rgba([
            (rgb[0] + dither).clamp(0.0, 255.0).round() as u8,
            (rgb[1] + dither).clamp(0.0, 255.0).round() as u8,
            (rgb[2] + dither).clamp(0.0, 255.0).round() as u8,
            255,
        ])
    })
}

pub struct Wallpaper {
    pub custom: bool,
    source: Option<RgbaImage>,
    dark: Option<bool>,
    clear: RgbaImage,
    material_source: Option<Arc<RgbaImage>>,
    prewarmed: Option<Arc<RenderImage>>,
    thumb: Option<Arc<RenderImage>>,
    tone_light: bool,
    contrast: bool,
    material_key: Option<(u16, u16)>,
}

fn material_sigma(source: &RgbaImage, tint: f32, viewport: Size<Pixels>) -> (f32, (u16, u16)) {
    let scale = (f32::from(viewport.width) / source.width() as f32)
        .max(f32::from(viewport.height) / source.height() as f32)
        .max(0.01);
    let css_blur = 3.0 + 22.0 * tint;
    let sigma = (css_blur / scale).clamp(0.75, 42.);
    let key = (
        (tint * 100.).round() as u16,
        (sigma * 4.).round().max(1.) as u16,
    );
    (sigma, key)
}

fn build_material(source: &RgbaImage, tint: f32, sigma: f32) -> Arc<RenderImage> {
    let saturate = 1.5 + 0.3 * tint;
    let (mut working, eff_sigma) = if tint >= 0.5 && source.width().max(source.height()) > 640 {
        let down = 640. / source.width().max(source.height()) as f32;
        let resized = image::imageops::resize(
            source,
            (source.width() as f32 * down).round().max(1.) as u32,
            (source.height() as f32 * down).round().max(1.) as u32,
            image::imageops::FilterType::Triangle,
        );
        (resized, (sigma * down).clamp(0.75, 42.))
    } else {
        (source.clone(), sigma)
    };
    for pixel in working.pixels_mut() {
        let y = 0.213 * f32::from(pixel[0])
            + 0.715 * f32::from(pixel[1])
            + 0.072 * f32::from(pixel[2]);
        for channel in &mut pixel.0[..3] {
            *channel = (y + (f32::from(*channel) - y) * saturate)
                .round()
                .clamp(0., 255.) as u8;
        }
    }
    let blurred = image::imageops::blur(&working, eff_sigma);
    render_image(blurred)
}

impl Wallpaper {
    pub fn load(file: Option<&Path>) -> Result<Self, String> {
        let source = file
            .map(|path| {
                image::open(path)
                    .map(|image| image.thumbnail(1440, 1000).into_rgba8())
                    .map_err(|e| format!("壁纸读取失败：{e}"))
            })
            .transpose()?;
        let mut wallpaper = Self {
            custom: source.is_some(),
            source,
            dark: None,
            clear: RgbaImage::default(),
            material_source: None,
            prewarmed: None,
            thumb: None,
            tone_light: true,
            contrast: false,
            material_key: None,
        };
        wallpaper.set_dark(false);
        Ok(wallpaper)
    }

    pub fn set_dark(&mut self, dark: bool) -> bool {
        if self.dark == Some(dark) {
            return false;
        }
        self.dark = Some(dark);
        let base: [u8; 4] = if dark {
            [0, 0, 0, 255]
        } else {
            [255, 255, 255, 255]
        };
        self.clear = match &self.source {
            Some(source) => RgbaImage::from_fn(source.width(), source.height(), |x, y| {
                let mut pixel = *source.get_pixel(x, y);
                let alpha = f32::from(pixel[3]) / 255. * if dark { 0.62 } else { 0.72 };
                for channel in 0..3 {
                    pixel[channel] = (f32::from(pixel[channel]) * alpha
                        + f32::from(base[channel]) * (1. - alpha))
                        .round() as u8;
                }
                pixel[3] = 255;
                pixel
            }),
            None => default_backdrop(dark),
        };
        self.material_key = None;
        self.prewarmed = None;
        if self.custom {
            let sample = image::imageops::thumbnail(&self.clear, 32, 32);
            let linear = |v: u8| {
                let c = f32::from(v) / 255.;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            let luminance = sample
                .pixels()
                .map(|v| 0.2126 * linear(v[0]) + 0.7152 * linear(v[1]) + 0.0722 * linear(v[2]))
                .sum::<f32>()
                / (sample.width() * sample.height()).max(1) as f32;
            self.tone_light = luminance > 0.179;
            self.contrast = self.tone_light == dark;
        } else {
            self.tone_light = !dark;
            self.contrast = false;
        }
        let factor = (1280. / self.clear.width().max(self.clear.height()) as f32).min(1.);
        let scaled = image::imageops::resize(
            &self.clear,
            (self.clear.width() as f32 * factor).round().max(1.) as u32,
            (self.clear.height() as f32 * factor).round().max(1.) as u32,
            image::imageops::FilterType::Triangle,
        );
        self.material_source = Some(Arc::new(scaled));
        self.thumb = self.source.as_ref().map(|src| {
            let preview = image::imageops::resize(
                src,
                96,
                56,
                image::imageops::FilterType::Triangle,
            );
            render_image(preview)
        });
        true
    }

    pub fn background(&self) -> Arc<RenderImage> {
        render_image(self.clear.clone())
    }

    pub fn prewarm_material(&mut self, tint: f32, viewport: Size<Pixels>) {
        let tint = tint.clamp(0., 1.);
        let Some(source) = self.material_source.as_ref() else {
            return;
        };
        let (sigma, key) = material_sigma(source, tint, viewport);
        self.prewarmed = Some(build_material(source, tint, sigma));
        self.material_key = Some(key);
    }
}

#[derive(Default)]
pub struct CardBackdrop {
    image: Option<Arc<RenderImage>>,
    source: Option<Arc<RgbaImage>>,
    thumb: Option<Arc<RenderImage>>,
    owner: Option<AnyWindowHandle>,
    custom: bool,
    tone_light: bool,
    contrast: bool,
    tint: f32,
}

impl Global for CardBackdrop {}

struct GlassGrain(bool);

impl Global for GlassGrain {}

impl crate::Overview {
    pub fn update_card_backdrop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tint = self.workspace.read(cx).glass_tint.clamp(0., 1.);
        let grain = tint >= 0.5;
        if cx
            .try_global::<GlassGrain>()
            .is_none_or(|current| current.0 != grain)
        {
            cx.set_global(GlassGrain(grain));
        }
        let source = self.wallpaper.material_source.clone();
        let owner = window.window_handle();
        let current = cx.try_global::<CardBackdrop>();
        let same_source = current.is_some_and(|c| match (&c.source, &source) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        });
        if !same_source {
            let stale = current.and_then(|c| c.image.clone());
            let stale_thumb = current.and_then(|c| c.thumb.clone());
            let initial = self.wallpaper.prewarmed.take();
            cx.set_global(CardBackdrop {
                source: source.clone(),
                thumb: self.wallpaper.thumb.clone(),
                owner: Some(owner),
                custom: self.wallpaper.custom,
                tone_light: self.wallpaper.tone_light,
                contrast: self.wallpaper.contrast,
                tint,
                image: initial,
            });
            if let Some(stale) = stale {
                cx.drop_image(stale, Some(window));
            }
            if let Some(stale_thumb) = stale_thumb {
                cx.drop_image(stale_thumb, Some(window));
            }
        } else if current.is_some_and(|b| {
            (b.tint - tint).abs() > 1e-4
                || b.custom != self.wallpaper.custom
                || b.tone_light != self.wallpaper.tone_light
                || b.contrast != self.wallpaper.contrast
        }) {
            let backdrop = cx.global_mut::<CardBackdrop>();
            backdrop.tint = tint;
            backdrop.thumb = self.wallpaper.thumb.clone();
            backdrop.custom = self.wallpaper.custom;
            backdrop.tone_light = self.wallpaper.tone_light;
            backdrop.contrast = self.wallpaper.contrast;
        }
        let Some(source) = source else { return };
        if self.material_busy {
            return;
        }
        let (sigma, key) = material_sigma(&source, tint, window.viewport_size());
        if self.wallpaper.material_key == Some(key)
            && cx.try_global::<CardBackdrop>().is_some_and(|b| b.image.is_some())
        {
            return;
        }
        self.material_busy = true;
        self.wallpaper.material_key = Some(key);
        let original = source.clone();
        let job = cx
            .background_executor()
            .spawn(async move { build_material(&source, tint, sigma) });
        cx.spawn_in(window, async move |weak, cx| {
            let image = job.await;
            let _ = cx.update(|window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.material_busy = false;
                    if view
                        .wallpaper
                        .material_source
                        .as_ref()
                        .is_some_and(|s| Arc::ptr_eq(s, &original))
                    {
                        let stale = cx.global_mut::<CardBackdrop>().image.replace(image);
                        if let Some(stale) = stale {
                            cx.drop_image(stale, Some(window));
                        }
                        window.refresh();
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }
}

pub fn card_shadows(dark: bool) -> Vec<BoxShadow> {
    if dark {
        vec![
            BoxShadow {
                color: hsla(0., 0., 0., 0.50),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(-10.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 1., 0.22),
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(-0.4),
                inset: true,
            },
        ]
    } else {
        vec![
            BoxShadow {
                color: hsla(0., 0., 0., 0.14),
                offset: point(px(0.), px(8.)),
                blur_radius: px(22.),
                spread_radius: px(-10.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 1., 0.78),
                offset: point(px(0.), px(1.2)),
                blur_radius: px(2.6),
                spread_radius: px(-0.6),
                inset: true,
            },
        ]
    }
}

pub fn card_hover_rim(dark: bool, hover: f32) -> gpui::Rgba {
    let h = hover.clamp(0., 1.);
    if dark {
        rgba(0xffffff00 | (0x24 + (20. * h).round() as u32))
    } else {
        rgba(0x00000000 | (0x18 + (12. * h).round() as u32))
    }
}

pub fn card_hover_shadows(dark: bool, hover: f32) -> Vec<BoxShadow> {
    let h = hover.clamp(0., 1.);
    if h <= 0.001 {
        return card_shadows(dark);
    }
    if dark {
        vec![
            BoxShadow {
                color: hsla(0., 0., 0., 0.35 * h),
                offset: point(px(0.), px(2.)),
                blur_radius: px(6.),
                spread_radius: px(0.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 0., 0.50 + 0.30 * h),
                offset: point(px(0.), px(8. + 10. * h)),
                blur_radius: px(24. + 16. * h),
                spread_radius: px(-10. - 2. * h),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 1., 0.22 + 0.12 * h),
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(-0.4),
                inset: true,
            },
        ]
    } else {
        vec![
            BoxShadow {
                color: hsla(0., 0., 0., 0.05 * h),
                offset: point(px(0.), px(2.)),
                blur_radius: px(4.),
                spread_radius: px(0.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 0., 0.14 + 0.12 * h),
                offset: point(px(0.), px(8. + 8. * h)),
                blur_radius: px(22. + 10. * h),
                spread_radius: px(-10. - 2. * h),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 1., 0.78 + 0.12 * h),
                offset: point(px(0.), px(1.2)),
                blur_radius: px(2.6),
                spread_radius: px(-0.6),
                inset: true,
            },
        ]
    }
}

fn hover_glow_sprite() -> &'static Arc<RenderImage> {
    static SPRITE: OnceLock<Arc<RenderImage>> = OnceLock::new();
    SPRITE.get_or_init(|| {
        const SIZE: u32 = 256;
        let half = (SIZE - 1) as f32 * 0.5;
        let image = RgbaImage::from_fn(SIZE, SIZE, |x, y| {
            let dx = (x as f32 - half) / half;
            let dy = (y as f32 - half) / half;
            let r = (dx * dx + dy * dy).sqrt().min(1.);
            let alpha = if r < 0.45 {
                let t = r / 0.45;
                let s = t * t * (3. - 2. * t);
                1.0 - 0.65 * s
            } else {
                let t = (r - 0.45) / 0.55;
                let s = t * t * (3. - 2. * t);
                0.35 * (1.0 - s)
            };
            Rgba([255, 255, 255, (alpha * 255.).round().clamp(0., 255.) as u8])
        });
        render_image(image)
    })
}

pub fn hover_glow_layer(
    pointer: Option<Point<Pixels>>,
    glow: f32,
    dark: bool,
    radius: Pixels,
) -> AnyElement {
    let g = glow.clamp(0., 1.);
    if g <= 0.005 {
        return div().into_any_element();
    }
    let glow_alpha = g * if dark { 0.11 } else { 0.38 };
    let rim_alpha = g * if dark { 0.16 } else { 0.45 };
    let rim = rgba(0xffffff00 | ((rim_alpha * 255.).round() as u32));
    div()
        .absolute()
        .inset_0()
        .size_full()
        .overflow_hidden()
        .rounded(radius)
        .child(
            div()
                .absolute()
                .inset_0()
                .size_full()
                .opacity(glow_alpha)
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let center = pointer.unwrap_or_else(|| {
                                point(
                                    bounds.origin.x + bounds.size.width * 0.5,
                                    bounds.origin.y + bounds.size.height * 0.5,
                                )
                            });
                            let r = px(200.);
                            let sprite_bounds = Bounds::new(
                                point(center.x - r, center.y - r),
                                size(r * 2., r * 2.),
                            );
                            let _ = window.paint_image(
                                sprite_bounds,
                                sprite_bounds,
                                Corners::all(px(0.)),
                                hover_glow_sprite().clone(),
                                0,
                                false,
                            );
                        },
                    )
                    .size_full(),
                ),
        )
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let r = radius
                        .min(bounds.size.width * 0.5)
                        .min(bounds.size.height * 0.5)
                        .max(px(0.));
                    window.paint_quad(quad(
                        bounds,
                        r,
                        transparent_black(),
                        px(1.),
                        rim,
                        BorderStyle::Solid,
                    ));
                },
            )
            .absolute()
            .inset_0()
            .size_full(),
        )
        .into_any_element()
}

pub fn float_shadows(dark: bool) -> Vec<BoxShadow> {
    if dark {
        vec![
            BoxShadow {
                color: hsla(0., 0., 0., 0.48),
                offset: point(px(0.), px(8.)),
                blur_radius: px(22.),
                spread_radius: px(-10.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 1., 0.24),
                offset: point(px(0.), px(1.)),
                blur_radius: px(2.),
                spread_radius: px(-0.4),
                inset: true,
            },
        ]
    } else {
        vec![
            BoxShadow {
                color: hsla(0., 0., 0., 0.15),
                offset: point(px(0.), px(8.)),
                blur_radius: px(22.),
                spread_radius: px(-10.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 1., 0.80),
                offset: point(px(0.), px(1.2)),
                blur_radius: px(2.6),
                spread_radius: px(-0.6),
                inset: true,
            },
        ]
    }
}

pub fn current_tint(cx: &App) -> f32 {
    cx.try_global::<CardBackdrop>().map_or(0.5, |b| b.tint)
}

pub fn wallpaper_thumb(cx: &App) -> Option<Arc<RenderImage>> {
    cx.try_global::<CardBackdrop>().and_then(|b| b.thumb.clone())
}

pub fn backdrop_ink(palette: crate::theme::Palette, cx: &App) -> gpui::Rgba {
    if let Some(backdrop) = cx.try_global::<CardBackdrop>()
        && backdrop.custom
    {
        return if backdrop.tone_light {
            rgb(0x1f2026)
        } else {
            rgb(0xfafafc)
        };
    }
    palette.ink()
}

pub fn backdrop_muted(palette: crate::theme::Palette, cx: &App) -> gpui::Rgba {
    if let Some(backdrop) = cx.try_global::<CardBackdrop>()
        && backdrop.custom
    {
        return if backdrop.tone_light {
            rgba(0x1f2026de)
        } else {
            rgba(0xfafafce0)
        };
    }
    palette.muted()
}

pub fn backdrop_text(
    text: impl Into<SharedString>,
    muted: bool,
    palette: crate::theme::Palette,
    cx: &App,
) -> Div {
    let text: SharedString = text.into();
    let ink = if muted {
        backdrop_muted(palette, cx)
    } else {
        backdrop_ink(palette, cx)
    };
    let halo = cx
        .try_global::<CardBackdrop>()
        .filter(|b| b.custom)
        .map(|b| {
            if b.tone_light {
                rgba(0xffffffcc)
            } else {
                rgba(0x00000099)
            }
        });
    div()
        .relative()
        .when_some(halo, |el, shadow| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(1.))
                    .text_color(shadow)
                    .child(text.clone()),
            )
        })
        .child(div().relative().text_color(ink).child(text))
}

pub fn backdrop_heading(
    text: impl Into<SharedString>,
    palette: crate::theme::Palette,
    cx: &App,
) -> Div {
    backdrop_text(text, false, palette, cx)
}

pub fn card_layer(wash: gpui::Rgba, radius: Pixels, minimum_alpha: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            let Some(backdrop) = cx.try_global::<CardBackdrop>() else {
                return;
            };
            let Some(image) = backdrop.image.as_ref() else {
                return;
            };
            if backdrop.owner != Some(window.window_handle()) {
                return;
            }
            let viewport = Bounds::new(point(px(0.), px(0.)), window.viewport_size());
            let cover = ObjectFit::Cover.get_bounds(viewport, image.size(0));
            let mut color = wash;
            if backdrop.contrast {
                color.a = color.a.max(minimum_alpha);
            }
            let lens = (1. - 2. * backdrop.tint).clamp(0., 1.);
            let w = f32::from(bounds.size.width);
            let h = f32::from(bounds.size.height);
            let r = f32::from(radius).min(w * 0.5).min(h * 0.5).max(0.);
            let clamped = px(r);
            let painted = if lens > 0.01 && w > 64. && h > 64. {
                let rim_h = 24.0f32.min(w * 0.25).min(h * 0.25);
                let max_disp = 24. * lens;
                let center_x = bounds.origin.x + bounds.size.width * 0.5;
                let center_y = bounds.origin.y + bounds.size.height * 0.5;
                const STEPS: usize = 10;
                let mut ok = false;
                for step in 0..=STEPS {
                    let frac = step as f32 / STEPS as f32;
                    let inset = rim_h * frac;
                    let t = 1. - frac;
                    let d = (1. - (1. - t * t).max(0.).sqrt()) * max_disp;
                    let sx = 1. + (2. * d) / w;
                    let sy = 1. + (2. * d) / h;
                    let shifted = Bounds::new(
                        point(
                            center_x - (center_x - cover.origin.x) * sx,
                            center_y - (center_y - cover.origin.y) * sy,
                        ),
                        size(cover.size.width * sx, cover.size.height * sy),
                    );
                    let ring = Bounds::new(
                        point(bounds.origin.x + px(inset), bounds.origin.y + px(inset)),
                        size(
                            (bounds.size.width - px(inset * 2.)).max(px(0.)),
                            (bounds.size.height - px(inset * 2.)).max(px(0.)),
                        ),
                    );
                    let ring_r = px((r - inset).max(0.));
                    if window
                        .paint_image(ring, shifted, ring_r.into(), image.clone(), 0, false)
                        .is_ok()
                    {
                        ok = true;
                    }
                }
                ok
            } else {
                window
                    .paint_image(bounds, cover, clamped.into(), image.clone(), 0, false)
                    .is_ok()
            };
            if painted {
                let dark = cx.global::<crate::theme::Appearance>().dark;
                let rim_alpha = if dark {
                    0.20 + 0.16 * lens
                } else {
                    0.56 + 0.30 * lens
                };
                let rim = rgba(0xffffff00 | ((rim_alpha * 255.).round() as u32));
                window.paint_quad(quad(
                    bounds,
                    clamped,
                    color,
                    px(1.),
                    rim,
                    BorderStyle::Solid,
                ));
                if lens > 0.05 && w > 24. && h > 24. {
                    let bevel_alpha = if dark { 0.14 * lens } else { 0.36 * lens };
                    let bevel = rgba(0xffffff00 | ((bevel_alpha * 255.).round() as u32));
                    let inner = Bounds::new(
                        point(bounds.origin.x + px(1.), bounds.origin.y + px(1.)),
                        size(bounds.size.width - px(2.), bounds.size.height - px(2.)),
                    );
                    window.paint_quad(quad(
                        inner,
                        px((r - 1.).max(0.)),
                        transparent_black(),
                        px(1.5),
                        bevel,
                        BorderStyle::Solid,
                    ));
                }
            }
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

pub fn control_layer(
    palette: crate::theme::Palette,
    radius: Pixels,
    extra_fill: bool,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            let Some(backdrop) = cx.try_global::<CardBackdrop>() else {
                return;
            };
            let Some(image) = backdrop.image.as_ref() else {
                return;
            };
            if backdrop.owner != Some(window.window_handle()) {
                return;
            }
            let viewport = Bounds::new(point(px(0.), px(0.)), window.viewport_size());
            let cover = ObjectFit::Cover.get_bounds(viewport, image.size(0));
            let mut alpha = 0.34 + 0.34 * backdrop.tint;
            if backdrop.contrast {
                alpha = alpha.max(0.66);
            }
            let wash = if palette.0 {
                rgba(0x2c2c3000 | ((alpha * 255.).round() as u32))
            } else {
                rgba(0xffffff00 | ((alpha * 255.).round() as u32))
            };
            let r = radius
                .min(bounds.size.width * 0.5)
                .min(bounds.size.height * 0.5)
                .max(px(0.));
            let bevel = if palette.0 {
                rgba(0xffffff1a)
            } else {
                rgba(0xffffff66)
            };
            if window
                .paint_image(bounds, cover, r.into(), image.clone(), 0, false)
                .is_ok()
            {
                window.paint_quad(quad(
                    bounds,
                    r,
                    wash,
                    px(1.),
                    bevel,
                    BorderStyle::Solid,
                ));
                if extra_fill {
                    window.paint_quad(quad(
                        bounds,
                        r,
                        palette.fill(),
                        px(0.),
                        transparent_black(),
                        BorderStyle::Solid,
                    ));
                }
            }
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

pub fn glass_input(
    id: impl Into<ElementId>,
    input: &Entity<gpui_component::input::InputState>,
    mono: bool,
    p: crate::theme::Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .overflow_hidden()
        .w_full()
        .min_w_0()
        .h(px(32.))
        .rounded(px(10.))
        .bg(p.fill())
        .border_1()
        .border_color(p.control_rim())
        .flex()
        .items_center()
        .when(mono, |el| el.font_family("Consolas"))
        .child(control_layer(p, px(9.), true))
        .child(
            div()
                .relative()
                .w_full()
                .min_w_0()
                .child(
                    gpui_component::input::Input::new(input)
                        .appearance(false)
                        .bordered(false)
                        .w_full(),
                ),
        )
}

pub fn bar_layer(
    palette: crate::theme::Palette,
    tint: f32,
    opacity: f32,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            if opacity <= 0.01 {
                return;
            }
            let Some(backdrop) = cx.try_global::<CardBackdrop>() else {
                return;
            };
            if backdrop.owner != Some(window.window_handle()) {
                return;
            }
            let mut base_alpha = 0.56 + 0.30 * tint.clamp(0., 1.);
            if backdrop.contrast {
                base_alpha = base_alpha.max(0.72);
            }
            let rgb_bits = if palette.0 { 0x2c2c3000 } else { 0xffffff00 };
            let total_h = f32::from(bounds.size.height);
            let fade_h = 26.0f32.min(total_h);
            let solid_h = (total_h - fade_h).max(0.);
            if solid_h > 0. {
                let mut wash = rgba(rgb_bits | 0xff);
                wash.a = base_alpha * opacity;
                window.paint_quad(quad(
                    Bounds::new(bounds.origin, size(bounds.size.width, px(solid_h))),
                    px(0.),
                    wash,
                    px(0.),
                    transparent_black(),
                    BorderStyle::Solid,
                ));
            }
            const BANDS: usize = 13;
            let band_h = fade_h / BANDS as f32;
            for i in 0..BANDS {
                let u = 1. - (i as f32 + 0.5) / BANDS as f32;
                let smooth = u * u * (3. - 2. * u);
                let mut wash = rgba(rgb_bits | 0xff);
                wash.a = base_alpha * opacity * smooth;
                let y = bounds.origin.y + px(solid_h + i as f32 * band_h);
                window.paint_quad(quad(
                    Bounds::new(point(bounds.origin.x, y), size(bounds.size.width, px(band_h + 0.5))),
                    px(0.),
                    wash,
                    px(0.),
                    transparent_black(),
                    BorderStyle::Solid,
                ));
            }
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

fn tiled_grain() -> &'static Arc<RenderImage> {
    static GRAIN: OnceLock<Arc<RenderImage>> = OnceLock::new();
    GRAIN.get_or_init(|| {
        let image = image::load_from_memory(include_bytes!("../../android/assets/glass_grain.png"))
            .expect("glass grain PNG")
            .into_rgba8();
        let tile = RgbaImage::from_fn(320, 320, |x, y| *image.get_pixel(x % 160, y % 160));
        render_image(tile)
    })
}

pub fn grain_layer(radius: Pixels) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            if !cx.try_global::<GlassGrain>().is_some_and(|grain| grain.0) {
                return;
            }
            let tile = tiled_grain();
            let step = |remaining: Pixels| {
                if remaining > px(320.) {
                    px(160.)
                } else {
                    remaining
                }
            };
            let mut y = px(0.);
            while y < bounds.size.height {
                let h = step(bounds.size.height - y);
                let bottom = y + h >= bounds.size.height;
                let mut x = px(0.);
                while x < bounds.size.width {
                    let w = step(bounds.size.width - x);
                    let right = x + w >= bounds.size.width;
                    let corner = |edge| if edge { radius } else { px(0.) };
                    let corners = Corners {
                        top_left: corner(x == px(0.) && y == px(0.)),
                        top_right: corner(right && y == px(0.)),
                        bottom_left: corner(x == px(0.) && bottom),
                        bottom_right: corner(right && bottom),
                    };
                    let origin = bounds.origin + point(x, y);
                    let _ = window.paint_image(
                        Bounds::new(origin, size(w, h)),
                        Bounds::new(origin, size(px(320.), px(320.))),
                        corners,
                        tile.clone(),
                        0,
                        false,
                    );
                    x += w;
                }
                y += h;
            }
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

pub fn render_image(mut image: RgbaImage) -> Arc<RenderImage> {
    for p in image.pixels_mut() {
        p.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![Frame::new(image)]))
}

#[cfg(test)]
mod tests {
    use super::{Wallpaper, default_backdrop, tiled_grain};

    #[test]
    fn grain_repeats_every_160_pixels_without_changing_its_alpha() {
        let tile = tiled_grain().as_bytes(0).unwrap();
        assert_eq!(tile.len(), 320 * 320 * 4);
        for y in 0..160 {
            let at = |x: usize, y: usize| &tile[(y * 320 + x) * 4..(y * 320 + x + 160) * 4];
            assert_eq!(at(0, y), at(160, y));
            assert_eq!(at(0, y), at(0, y + 160));
        }
        let alpha = tile.chunks_exact(4).map(|pixel| pixel[3]);
        assert_eq!((alpha.clone().min(), alpha.max()), (Some(1), Some(10)));
    }

    #[test]
    fn default_backdrop_renders_distinct_light_and_dark_auroras() {
        let light = default_backdrop(false);
        let dark = default_backdrop(true);
        assert_eq!(light.dimensions(), (960, 600));
        assert_eq!(dark.dimensions(), (960, 600));

        let l_tl = light.get_pixel(80, 80).0;
        let l_br = light.get_pixel(820, 500).0;
        assert_ne!(l_tl, l_br);
        assert!(l_tl[2] > l_tl[0]);

        let d_tl = dark.get_pixel(80, 80).0;
        let d_center = dark.get_pixel(480, 300).0;
        assert_ne!(d_tl, d_center);
        assert!(
            u16::from(l_tl[0]) + u16::from(l_tl[1])
                > u16::from(d_tl[0]) + u16::from(d_tl[1]) + 200
        );

        let mut wp = Wallpaper::load(None).unwrap();
        assert!(!wp.custom);
        assert!(wp.set_dark(true));
        assert!(!wp.set_dark(true));
        assert!(wp.set_dark(false));
    }
}
