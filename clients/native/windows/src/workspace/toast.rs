use gpui::{prelude::*, *};
use std::time::{Duration, Instant};

const LIFETIME: Duration = Duration::from_secs(5);
const EXIT: Duration = Duration::from_millis(260);

struct Toast {
    id: usize,
    text: String,
    error: bool,
    started: Instant,
    expires: Option<Instant>,
    remaining: Duration,
    closing: Option<Instant>,
}

impl Toast {
    fn remaining(&self, now: Instant) -> Duration {
        self.expires.map_or(self.remaining, |expires| {
            expires.saturating_duration_since(now)
        })
    }

    fn hover(&mut self, hovered: bool, now: Instant) {
        if self.closing.is_some() {
            return;
        }
        if hovered && self.expires.is_some() {
            self.remaining = self.remaining(now);
            self.expires = None;
            if self.remaining.is_zero() {
                self.closing = Some(now);
            }
        } else if !hovered && self.expires.is_none() {
            self.expires = Some(now + self.remaining);
        }
    }
}

#[derive(Default)]
pub(crate) struct ToastHost {
    entries: Vec<Toast>,
    sequence: usize,
}

impl ToastHost {
    pub(super) fn push(&mut self, text: String, error: bool, cx: &mut Context<Self>) {
        self.enqueue(text, error, Instant::now());
        cx.notify();
    }

    fn enqueue(&mut self, text: String, error: bool, now: Instant) {
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.push(Toast {
            id: self.sequence,
            text,
            error,
            started: now,
            expires: Some(now + LIFETIME),
            remaining: LIFETIME,
            closing: None,
        });
    }
}

impl Render for ToastHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let reduce_motion = cx.reduce_motion();
        for entry in &mut self.entries {
            if entry.closing.is_none() && entry.expires.is_some_and(|t| now >= t) {
                entry.closing = entry.expires;
            }
        }
        self.entries.retain(|entry| {
            !entry
                .closing
                .is_some_and(|t| reduce_motion || now.saturating_duration_since(t) >= EXIT)
        });
        let p = crate::theme::Palette::get(cx);
        let mut host = div()
            .id("panel-notifications")
            .role(Role::Status)
            .absolute()
            .right(px(16.))
            .bottom(px(16.))
            .flex()
            .flex_col()
            .items_end()
            .gap_2();
        for entry in &self.entries {
            let id = entry.id;
            let age = now
                .saturating_duration_since(entry.closing.unwrap_or(entry.started))
                .as_secs_f32();
            let progress = (age / 0.260).min(1.);
            let position = super::dialog::pop_easing(progress);
            let alpha = (age / 0.220).min(1.);
            let (position, alpha) = if reduce_motion {
                (1., 1.)
            } else if entry.closing.is_some() {
                (1. - position, 1. - alpha)
            } else {
                (position, alpha)
            };
            let scale = 0.96 + 0.04 * position;
            let accent = if entry.error { p.danger() } else { p.accent() };
            let remaining = entry.remaining(now).as_secs_f32() / LIFETIME.as_secs_f32();
            host = host.child(
                div()
                    .id(("panel-toast", id))
                    .occlude()
                    .relative()
                    .top(px((1. - position) * 8.))
                    .min_w(px(260. * scale))
                    .max_w(px(420. * scale))
                    .px(px(14. * scale))
                    .pt(px(12. * scale))
                    .pb(px(10. * scale))
                    .rounded(px(18. * scale))
                    .overflow_hidden()
                    .border_1()
                    .border_color(p.rim())
                    .bg(p.popover(1.))
                    .shadow_lg()
                    .opacity(alpha)
                    .child(crate::glass::card_layer(p.popover(1.), px(17. * scale), 0.78))
                    .child(crate::glass::grain_layer(px(17. * scale)))
                    .on_hover(cx.listener(move |v, hovered: &bool, _, cx| {
                        if let Some(item) = v.entries.iter_mut().find(|item| item.id == id) {
                            item.hover(*hovered, Instant::now());
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(10. * scale))
                            .child(
                                div()
                                    .flex_none()
                                    .mt(px(2. * scale))
                                    .size(px(20. * scale))
                                    .rounded_full()
                                    .bg(accent)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        svg()
                                            .path(if entry.error {
                                                "toast-error"
                                            } else {
                                                "toast-info"
                                            })
                                            .size(px(12. * scale))
                                            .text_color(rgb(0xffffff)),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .text_size(crate::theme::text_px(14. * scale))
                                    .line_height(rems(19. * scale / 16.))
                                    .text_color(p.ink())
                                    .child(crate::emoji::text(entry.text.clone())),
                            )
                            .child(
                                div()
                                    .id(("toast-close", id))
                                    .role(Role::Button)
                                    .aria_label("关闭")
                                    .size(px(20. * scale))
                                    .rounded_full()
                                    .cursor_pointer()
                                    .hover(|d| d.bg(p.hover()))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        svg()
                                            .path("x-small")
                                            .size(px(10. * scale))
                                            .text_color(p.muted()),
                                    )
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        if let Some(item) =
                                            v.entries.iter_mut().find(|item| item.id == id)
                                        {
                                            item.closing.get_or_insert(Instant::now());
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(10. * scale))
                            .h(px(3.))
                            .w_full()
                            .rounded_full()
                            .bg(p.fill())
                            .child(
                                div()
                                    .rounded_full()
                                    .h_full()
                                    .w(relative(remaining.clamp(0., 1.)))
                                    .bg(accent),
                            ),
                    ),
            );
            if entry.expires.is_some() || entry.closing.is_some() || progress < 1. {
                window.request_animation_frame();
            }
        }
        host
    }
}

#[cfg(test)]
mod tests {
    use super::{Duration, Instant, LIFETIME, ToastHost};

    #[test]
    fn identical_messages_are_separate_and_hover_preserves_each_lifetime() {
        let now = Instant::now();
        let mut host = ToastHost::default();
        host.enqueue("已保存".into(), false, now);
        host.enqueue("已保存".into(), true, now);
        assert_ne!(host.entries[0].id, host.entries[1].id);
        host.entries[0].hover(true, now + Duration::from_secs(2));
        assert_eq!(host.entries[0].remaining(now + LIFETIME), Duration::from_secs(3));
        assert_eq!(host.entries[1].remaining(now + LIFETIME), Duration::ZERO);
        host.entries[0].hover(false, now + LIFETIME);
        assert_eq!(host.entries[0].expires, Some(now + Duration::from_secs(8)));
        host.entries[0].closing = Some(now + LIFETIME);
        host.entries[0].hover(true, now + LIFETIME);
        assert!(host.entries[0].expires.is_some());
    }
}
