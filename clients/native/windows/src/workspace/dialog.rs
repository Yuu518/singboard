use super::*;
use gpui_component::FocusTrapElement;
use std::time::{Duration, Instant};

const TRANSITION: Duration = Duration::from_millis(220);

pub(super) enum LeavingDialog {
    Settings(settings::SettingsDialog),
    Config(config::ConfigDialog),
    Update(settings::PanelUpdate),
    Connection(Value),
    Disconnect(Command),
    Rule(rules::RuleDetail),
}

impl Workspace {
    pub(crate) fn page_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let visible = match self.page {
            Page::Connections => self.detail.is_some() || self.confirm.is_some(),
            Page::Rules => self.rule_browser.detail.is_some(),
            Page::Config => self.config_dialog.is_some(),
            Page::Settings => self.settings_layout.dialog.is_some(),
            _ => false,
        };
        if visible && self.page_dialog_focus.is_none() {
            let scope = cx.focus_handle();
            let previous = window.focused(cx);
            let initial = self.rule_browser.detail.as_ref().map(|d| d.search.read(cx).focus.clone());
            if !matches!(self.page, Page::Config) || previous.is_none() {
                window.focus(initial.as_ref().unwrap_or(&scope), cx);
            }
            self.page_dialog_focus = Some((scope, previous));
        } else if !visible && self.leaving_dialog.is_some() {
            if let Some((scope, _)) = &self.page_dialog_focus {
                window.focus(scope, cx);
            }
        } else if !visible {
            if let Some((scope, Some(previous))) = self.page_dialog_focus.take()
                && (scope.contains_focused(window, cx) || window.focused(cx).is_none())
            {
                window.focus(&previous, cx);
            }
        }
        let overlay = match self.page {
            Page::Connections => self.connection_overlay(window, cx),
            Page::Rules => self.rule_overlay(window, cx),
            Page::Config => self.render_config_dialog(cx).map(|el| div().absolute().inset_0().child(el)),
            Page::Settings => self.render_settings_dialog(cx).map(|el| div().absolute().inset_0().child(el)),
            _ => None,
        }?;
        Some(match &self.page_dialog_focus {
            Some((scope, _)) => overlay.tab_group().tab_stop(false).focus_trap("data-dialog-focus", scope).into_any_element(),
            None => overlay.into_any_element(),
        })
    }

    pub(super) fn leave_dialog(&mut self, content: LeavingDialog, cx: &mut Context<Self>) {
        if cx.reduce_motion() {
            self.leaving_dialog = None;
        } else {
            let now = Instant::now();
            self.leaving_dialog = Some((now, content));
            cx.spawn(async move |weak, cx| {
                cx.background_executor().timer(TRANSITION).await;
                let _ = weak.update(cx, |this, cx| {
                    if this.leaving_dialog.as_ref().is_some_and(|(start, _)| *start == now) {
                        this.leaving_dialog = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    pub(super) fn close_settings_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(content) = self.settings_layout.dialog.take() {
            self.leave_dialog(LeavingDialog::Settings(content), cx);
        }
    }

    pub(super) fn close_config_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(content) = self.config_dialog.take() {
            self.leave_dialog(LeavingDialog::Config(content), cx);
        }
    }

    pub(super) fn close_update_dialog(&mut self, content: settings::PanelUpdate, cx: &mut Context<Self>) {
        self.leave_dialog(LeavingDialog::Update(content), cx);
    }
}

pub(super) fn keyboard_action(button: Stateful<Div>, action: Action, cx: &Context<Workspace>) -> Stateful<Div> {
    let palette = crate::theme::Palette::get(cx);
    button.tab_index(0)
        .focus_visible(|s| s.bg(palette.primary_soft()))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") && this.leaving_dialog.is_none() {
                this.apply(action.clone(), cx);
                cx.stop_propagation();
            }
        }))
}

pub(super) fn surface(
    id: &'static str,
    scrim: Stateful<Div>,
    card: impl Styled + IntoElement + 'static,
    width: f32,
    leaving: bool,
    cx: &App,
) -> AnyElement {
    let scrim = scrim.occlude();
    if cx.reduce_motion() {
        return scrim.child(card).into_any_element();
    }
    let card = card.with_animation(
        (id, usize::from(leaving)),
        Animation::new(TRANSITION).with_easing(pop_easing),
        move |card, t| {
            let scale = 0.94 + 0.06 * if leaving { 1. - t } else { t };
            card.w(px(width * scale)).max_w_full().p(px(20. * scale))
        },
    );
    scrim.child(card)
        .when(leaving, |s| s.child(div().absolute().inset_0().occlude()))
        .with_animation(
            (id, usize::from(leaving) + 2),
            Animation::new(TRANSITION),
            move |scrim, t| {
                let elapsed = (t * 220. / 180.).min(1.);
                let opacity = elapsed * elapsed * (3. - 2. * elapsed);
                scrim.opacity(if leaving { 1. - opacity } else { opacity })
            },
        )
        .into_any_element()
}

pub(super) fn pop_easing(progress: f32) -> f32 {
    if progress <= 0. || progress >= 1. {
        return progress;
    }
    let mut low = 0.;
    let mut high = 1.;
    for _ in 0..12 {
        let t = (low + high) * 0.5;
        let u = 1. - t;
        let x = 3. * u * u * t * 0.32 + t * t * t;
        if x < progress {
            low = t;
        } else {
            high = t;
        }
    }
    let t = (low + high) * 0.5;
    let u = 1. - t;
    3. * u * u * t * 0.72 + 3. * u * t * t + t * t * t
}
