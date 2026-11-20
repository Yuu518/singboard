use super::*;
use gpui_component::scroll::ScrollableElement;
use std::time::Instant;

pub(super) struct ScrollMotion {
    start: f32,
    target: f32,
    started: Instant,
    duration: f32,
    expected_last: Option<f32>,
}

impl ScrollMotion {
    pub(super) fn new(start: f32, target: f32, duration: f32) -> Self {
        Self {
            start,
            target,
            started: Instant::now(),
            duration: duration.max(0.01),
            expected_last: Some(start),
        }
    }
}

pub(super) fn cubic_bezier_y(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    if x <= 0. {
        return 0.;
    }
    if x >= 1. {
        return 1.;
    }
    let sample_x = |t: f32| {
        let u = 1. - t;
        3. * u * u * t * x1 + 3. * u * t * t * x2 + t * t * t
    };
    let sample_y = |t: f32| {
        let u = 1. - t;
        3. * u * u * t * y1 + 3. * u * t * t * y2 + t * t * t
    };
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    let mut t = x;
    for _ in 0..10 {
        let cur = sample_x(t);
        if (cur - x).abs() < 1e-4 {
            break;
        }
        if cur < x {
            lo = t;
        } else {
            hi = t;
        }
        t = 0.5 * (lo + hi);
    }
    sample_y(t)
}

pub(super) fn ease_ios(t: f32) -> f32 {
    cubic_bezier_y(0.32, 0.72, 0., 1., t)
}

pub(super) fn ease_spring_pop(t: f32) -> f32 {
    cubic_bezier_y(0.34, 1.56, 0.64, 1., t)
}

pub(super) fn ease_standard(t: f32) -> f32 {
    cubic_bezier_y(0.25, 0.1, 0.25, 1., t)
}

pub(super) fn nearest_target(
    current_y: f32,
    max_y: f32,
    viewport_top: f32,
    viewport_bottom: f32,
    item_top: f32,
    item_bottom: f32,
    margin: f32,
) -> Option<f32> {
    if max_y <= 0.5 || viewport_bottom <= viewport_top {
        return None;
    }
    let delta = if item_top >= viewport_top && item_bottom <= viewport_bottom {
        return None;
    } else if item_bottom > viewport_bottom {
        (viewport_bottom - item_bottom) - margin
    } else {
        (viewport_top - item_top) + margin
    };
    let next = (current_y + delta).clamp(-max_y, 0.);
    ((next - current_y).abs() >= 1.0).then_some(next)
}

impl Workspace {
    pub(super) fn advance_scroll(&mut self, window: &Window) {
        let Some(motion) = self.scroll_motion.as_mut() else {
            return;
        };
        let current = f32::from(self.page_scroll.offset().y);
        if let Some(expected) = motion.expected_last
            && (current - expected).abs() > 2.5
        {
            self.scroll_motion = None;
            return;
        }
        let max_y = f32::from(self.page_scroll.max_offset().y).max(0.);
        let target = motion.target.clamp(-max_y, 0.);
        motion.target = target;
        let t = (motion.started.elapsed().as_secs_f32() / motion.duration).clamp(0., 1.);
        let next = (motion.start + (target - motion.start) * ease_ios(t)).clamp(-max_y, 0.);
        self.page_scroll.set_offset(point(px(0.), px(next)));
        motion.expected_last = Some(next);
        if t < 1.0 && (target - next).abs() > 0.3 {
            window.request_animation_frame();
        } else {
            self.page_scroll.set_offset(point(px(0.), px(target)));
            self.scroll_motion = None;
        }
    }

    pub(super) fn handle_smooth_wheel(&mut self, event: &ScrollEvent) -> bool {
        match event.delta {
            ScrollDelta::Pixels(_) => {
                self.scroll_motion = None;
                false
            }
            ScrollDelta::Lines(delta) => {
                if delta.y.abs() <= 0.001 {
                    return false;
                }
                let max_y = f32::from(self.page_scroll.max_offset().y).max(0.);
                if max_y <= 0.5 {
                    return false;
                }
                let current = f32::from(self.page_scroll.offset().y);
                let base = self.scroll_motion.as_ref().map_or(current, |m| m.target);
                let step = delta.y * 44.;
                let target = (base + step).clamp(-max_y, 0.);
                if (target - current).abs() < 0.5 {
                    self.scroll_motion = None;
                    return false;
                }
                self.scroll_motion = Some(ScrollMotion::new(current, target, 0.17));
                true
            }
        }
    }

    pub(super) fn scroll_bounds_nearest(&mut self, item: Bounds<Pixels>) {
        let viewport = self.page_scroll.bounds();
        let max_y = f32::from(self.page_scroll.max_offset().y).max(0.);
        let current = f32::from(self.page_scroll.offset().y);
        if let Some(target) = nearest_target(
            current,
            max_y,
            f32::from(viewport.top()),
            f32::from(viewport.bottom()),
            f32::from(item.top()),
            f32::from(item.bottom()),
            12.,
        ) {
            self.scroll_motion = Some(ScrollMotion::new(current, target, 0.24));
        }
    }

    pub(super) fn scroll_pane(
        &self,
        id: &'static str,
        body: impl IntoElement,
        cx: &Context<Self>,
    ) -> Div {
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .id(id)
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.page_scroll)
                    .child(
                        div()
                            .w_full()
                            .min_h_full()
                            .px(px(20.))
                            .pb(px(16.))
                            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                                if this.handle_smooth_wheel(event) {
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            }))
                            .child(body),
                    ),
            )
            .vertical_scrollbar(&self.page_scroll)
    }
}

#[cfg(test)]
mod tests {
    use super::{ease_ios, ease_spring_pop, nearest_target};

    #[test]
    fn nearest_scroll_never_moves_visible_items_and_uses_minimal_delta() {
        assert_eq!(nearest_target(-100., 500., 50., 650., 120., 196., 12.), None);
        assert_eq!(
            nearest_target(-100., 500., 50., 650., 620., 696., 12.),
            Some(-158.)
        );
        assert_eq!(
            nearest_target(-200., 500., 50., 650., 10., 86., 12.),
            Some(-148.)
        );
        assert_eq!(
            nearest_target(-480., 500., 50., 650., 640., 760., 12.),
            Some(-500.)
        );
    }

    #[test]
    fn easing_curves_hit_endpoints_and_spring_overshoots() {
        assert_eq!(ease_ios(0.), 0.);
        assert_eq!(ease_ios(1.), 1.);
        assert!(ease_ios(0.5) > 0.7);
        assert!(ease_spring_pop(0.65) > 1.0);
        assert_eq!(ease_spring_pop(1.), 1.);
    }
}
