use super::*;
use gpui_component::scroll::ScrollableElement;
use std::time::Instant;

const WHEEL_OMEGA: f32 = 15.0;
const REVEAL_OMEGA: f32 = 14.0;
const OVERSCROLL_OMEGA: f32 = 18.0;
const MAX_OVERSCROLL: f32 = 24.0;
const WHEEL_LINE_PX: f32 = 40.0;
const STREAK_WINDOW: f32 = 0.15;
const STREAK_STEP: f32 = 0.18;
const STREAK_MAX: f32 = 0.65;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ScrollTarget {
    Page,
    PageList,
    Logs,
    ConnDetail,
    RuleDetail,
}

pub(super) struct SmoothScroll {
    target_kind: ScrollTarget,
    position: f32,
    velocity: f32,
    target: f32,
    overscroll: f32,
    overscroll_velocity: f32,
    omega: f32,
    last_tick: Instant,
    last_wheel: Option<Instant>,
    last_dir: f32,
    streak: f32,
    expected_last: Option<f32>,
}

impl SmoothScroll {
    fn new(target_kind: ScrollTarget, current: f32, target: f32, omega: f32) -> Self {
        let now = Instant::now();
        Self {
            target_kind,
            position: current,
            velocity: 0.0,
            target,
            overscroll: 0.0,
            overscroll_velocity: 0.0,
            omega: omega.max(1.0),
            last_tick: now,
            last_wheel: None,
            last_dir: 0.0,
            streak: 0.0,
            expected_last: Some(current),
        }
    }
}

pub(super) fn step_spring(
    value: f32,
    velocity: f32,
    target: f32,
    omega: f32,
    dt: f32,
) -> (f32, f32) {
    let d = value - target;
    let e = (-omega * dt).exp();
    let k = velocity + omega * d;
    let next_d = e * (d + k * dt);
    let next_v = e * (velocity - omega * k * dt);
    (target + next_d, next_v)
}

pub(super) fn damped_overscroll(current: f32, excess: f32) -> (f32, f32) {
    let headroom = (1.0 - current.abs() / MAX_OVERSCROLL).clamp(0.08, 1.0);
    let impulse = (excess.clamp(-120.0, 120.0) * 0.14) * headroom;
    let next = (current + impulse).clamp(-MAX_OVERSCROLL, MAX_OVERSCROLL);
    (next, impulse * 4.0)
}

pub(super) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    fn scroll_metrics(&self, kind: ScrollTarget) -> Option<(f32, f32)> {
        match kind {
            ScrollTarget::Page => Some((
                f32::from(self.page_scroll.offset().y),
                f32::from(self.page_scroll.max_offset().y).max(0.),
            )),
            ScrollTarget::PageList => {
                let state = self.page_list_scroll.0.borrow();
                Some((
                    f32::from(state.base_handle.offset().y),
                    f32::from(state.base_handle.max_offset().y).max(0.),
                ))
            }
            ScrollTarget::Logs => Some((
                f32::from(self.log_list.scroll_px_offset_for_scrollbar().y),
                f32::from(self.log_list.max_offset_for_scrollbar().y).max(0.),
            )),
            ScrollTarget::ConnDetail => Some((
                f32::from(self.conn_detail_scroll.offset().y),
                f32::from(self.conn_detail_scroll.max_offset().y).max(0.),
            )),
            ScrollTarget::RuleDetail => {
                let detail = self.rule_browser.detail.as_ref()?;
                let state = detail.scroll.0.borrow();
                Some((
                    f32::from(state.base_handle.offset().y),
                    f32::from(state.base_handle.max_offset().y).max(0.),
                ))
            }
        }
    }

    fn apply_scroll_offset(&mut self, kind: ScrollTarget, y: f32) {
        match kind {
            ScrollTarget::Page => self.page_scroll.set_offset(point(px(0.), px(y))),
            ScrollTarget::PageList => {
                self.page_list_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), px(y)));
            }
            ScrollTarget::Logs => {
                self.log_list.set_offset_from_scrollbar(point(px(0.), px(y)));
                self.log_auto_scroll = self.log_list.is_following_tail();
            }
            ScrollTarget::ConnDetail => self.conn_detail_scroll.set_offset(point(px(0.), px(y))),
            ScrollTarget::RuleDetail => {
                if let Some(detail) = self.rule_browser.detail.as_ref() {
                    detail
                        .scroll
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(point(px(0.), px(y)));
                }
            }
        }
    }

    pub(super) fn cancel_scroll(&mut self, kind: ScrollTarget) {
        if self
            .scroll_motion
            .as_ref()
            .is_some_and(|m| m.target_kind == kind)
        {
            self.scroll_motion = None;
        }
    }

    pub(super) fn overscroll_offset(&self, kind: ScrollTarget) -> Pixels {
        let y = self
            .scroll_motion
            .as_ref()
            .filter(|m| m.target_kind == kind)
            .map_or(0.0, |m| m.overscroll);
        px(y)
    }

    pub(super) fn advance_scroll(&mut self, window: &Window) {
        let Some(kind) = self.scroll_motion.as_ref().map(|m| m.target_kind) else {
            return;
        };
        let Some((current, max_y)) = self.scroll_metrics(kind) else {
            self.scroll_motion = None;
            return;
        };
        let Some(motion) = self.scroll_motion.as_mut() else {
            return;
        };
        if let Some(expected) = motion.expected_last
            && (current - expected).abs() > 2.5
        {
            self.scroll_motion = None;
            return;
        }
        let now = Instant::now();
        let dt = now
            .duration_since(motion.last_tick)
            .as_secs_f32()
            .clamp(0.0005, 0.05);
        motion.last_tick = now;
        motion.target = motion.target.clamp(-max_y, 0.0);

        let (mut next, mut next_v) = step_spring(
            motion.position,
            motion.velocity,
            motion.target,
            motion.omega,
            dt,
        );
        if next > 0.0 || next < -max_y {
            let impact = (next_v * 0.015).clamp(-10.0, 10.0);
            let (ov, ov_v) = damped_overscroll(motion.overscroll, impact * 4.0);
            motion.overscroll = ov;
            motion.overscroll_velocity = ov_v;
            next = next.clamp(-max_y, 0.0);
            next_v = 0.0;
        }
        motion.position = next;
        motion.velocity = next_v;

        let (next_over, next_over_v) = step_spring(
            motion.overscroll,
            motion.overscroll_velocity,
            0.0,
            OVERSCROLL_OMEGA,
            dt,
        );
        motion.overscroll = next_over;
        motion.overscroll_velocity = next_over_v;

        let main_settled =
            (motion.target - motion.position).abs() < 0.25 && motion.velocity.abs() < 4.0;
        let over_settled =
            motion.overscroll.abs() < 0.15 && motion.overscroll_velocity.abs() < 3.0;
        let apply_y = if main_settled {
            motion.position = motion.target;
            motion.velocity = 0.0;
            motion.target
        } else {
            motion.position
        };
        if over_settled {
            motion.overscroll = 0.0;
            motion.overscroll_velocity = 0.0;
        }
        motion.expected_last = Some(apply_y);
        let done = main_settled && over_settled;
        self.apply_scroll_offset(kind, apply_y);
        if done {
            self.scroll_motion = None;
        } else {
            window.request_animation_frame();
        }
    }

    pub(super) fn handle_smooth_wheel_for(
        &mut self,
        kind: ScrollTarget,
        event: &ScrollEvent,
    ) -> bool {
        match event.delta {
            ScrollDelta::Pixels(_) => {
                self.cancel_scroll(kind);
                false
            }
            ScrollDelta::Lines(delta) => {
                if delta.y.abs() <= 0.001 {
                    return false;
                }
                let Some((current, max_y)) = self.scroll_metrics(kind) else {
                    return false;
                };
                if max_y <= 0.5 {
                    return false;
                }
                let now = Instant::now();
                let dir = delta.y.signum();
                let reusing = self.scroll_motion.as_ref().is_some_and(|m| {
                    m.target_kind == kind
                        && m.expected_last
                            .is_none_or(|expected| (current - expected).abs() <= 2.5)
                });
                let (mut position, mut velocity, mut overscroll, mut overscroll_velocity, streak) =
                    if reusing {
                        let m = self.scroll_motion.as_ref().unwrap();
                        let continuing = m.last_dir == dir
                            && m.last_wheel
                                .is_some_and(|t| now.duration_since(t).as_secs_f32() < STREAK_WINDOW);
                        let next_streak = if continuing {
                            (m.streak + STREAK_STEP).min(STREAK_MAX)
                        } else {
                            0.0
                        };
                        (
                            current,
                            m.velocity,
                            m.overscroll,
                            m.overscroll_velocity,
                            next_streak,
                        )
                    } else {
                        (current, 0.0, 0.0, 0.0, 0.0)
                    };

                let step = delta.y * WHEEL_LINE_PX * (1.0 + streak);
                let base = if reusing {
                    let prev_target = self.scroll_motion.as_ref().unwrap().target;
                    if (prev_target - current) * step < 0.0 {
                        velocity *= 0.25;
                        overscroll = 0.0;
                        overscroll_velocity = 0.0;
                        current
                    } else {
                        prev_target
                    }
                } else {
                    current
                };
                let raw_target = base + step;
                let target = raw_target.clamp(-max_y, 0.0);
                let excess = raw_target - target;
                if excess.abs() > 0.5 {
                    let (next_ov, next_ov_v) = damped_overscroll(overscroll, excess);
                    overscroll = next_ov;
                    overscroll_velocity = next_ov_v;
                }
                if (target - current).abs() < 0.5 && overscroll.abs() < 0.25 {
                    self.cancel_scroll(kind);
                    return false;
                }
                position = position.clamp(-max_y, 0.0);
                self.scroll_motion = Some(SmoothScroll {
                    target_kind: kind,
                    position,
                    velocity,
                    target,
                    overscroll,
                    overscroll_velocity,
                    omega: WHEEL_OMEGA,
                    last_tick: now,
                    last_wheel: Some(now),
                    last_dir: dir,
                    streak,
                    expected_last: Some(current),
                });
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
            let mut motion = SmoothScroll::new(ScrollTarget::Page, current, target, REVEAL_OMEGA);
            if let Some(prev) = self
                .scroll_motion
                .as_ref()
                .filter(|m| m.target_kind == ScrollTarget::Page)
            {
                motion.velocity = prev.velocity;
            }
            self.scroll_motion = Some(motion);
        }
    }

    pub(super) fn smooth_scroll_capture(&self, kind: ScrollTarget, cx: &Context<Self>) -> Div {
        div()
            .absolute()
            .inset_0()
            .on_scroll(cx.listener(move |this, event: &ScrollEvent, _, cx| {
                if this.handle_smooth_wheel_for(kind, event) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
    }

    pub(super) fn scroll_pane(
        &self,
        id: &'static str,
        body: impl IntoElement,
        cx: &Context<Self>,
    ) -> Div {
        let top_inset = match self.page {
            Page::Overview | Page::Settings => px(48.),
            _ => px(0.),
        };
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
                            .relative()
                            .top(self.overscroll_offset(ScrollTarget::Page))
                            .w_full()
                            .min_h_full()
                            .px(px(20.))
                            .pb(px(16.))
                            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                                if this.handle_smooth_wheel_for(ScrollTarget::Page, event) {
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            }))
                            .child(body),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(top_inset)
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .vertical_scrollbar(&self.page_scroll),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_OVERSCROLL, WHEEL_OMEGA, damped_overscroll, ease_ios, ease_spring_pop, nearest_target,
        smoothstep, step_spring,
    };

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

    #[test]
    fn critically_damped_spring_converges_monotonically_without_overshoot() {
        let mut x = 0.0f32;
        let mut v = 0.0f32;
        let target = -120.0f32;
        for _ in 0..40 {
            let (nx, nv) = step_spring(x, v, target, WHEEL_OMEGA, 1.0 / 60.0);
            assert!(nx <= x + 1e-4);
            assert!(nx >= target - 1e-4);
            x = nx;
            v = nv;
        }
        assert!((x - target).abs() < 0.5);
    }

    #[test]
    fn overscroll_is_bounded_and_smoothstep_clamps() {
        let mut ov = 0.0f32;
        for _ in 0..20 {
            let (next, _) = damped_overscroll(ov, 120.0);
            assert!(next >= ov);
            assert!(next <= MAX_OVERSCROLL);
            ov = next;
        }
        assert_eq!(smoothstep(0., 32., -4.), 0.);
        assert_eq!(smoothstep(0., 32., 16.), 0.5);
        assert_eq!(smoothstep(0., 32., 48.), 1.);
    }
}
