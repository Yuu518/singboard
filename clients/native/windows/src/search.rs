use gpui::{prelude::*, *};
use std::ops::Range;

pub struct Search {
    pub text: String,
    pub placeholder: SharedString,
    pub focus: FocusHandle,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
}

impl Search {
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.text.clear();
        self.selection = 0..0;
        self.marked = None;
        cx.notify();
    }
    pub fn set_placeholder(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let next = text.into();
        if self.placeholder != next {
            self.placeholder = next;
            cx.notify();
        }
    }
    pub fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        self.selection = 0..self.text.encode_utf16().count();
        cx.notify();
    }
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            placeholder: "搜索（Ctrl+F）".into(),
            focus: cx.focus_handle(),
            selection: 0..0,
            marked: None,
        }
    }
    fn bytes(&self, range: Range<usize>) -> Range<usize> {
        let index = |n| {
            let mut units = 0;
            for (i, c) in self.text.char_indices() {
                if units + c.len_utf16() > n {
                    return i;
                }
                units += c.len_utf16();
            }
            self.text.len()
        };
        index(range.start)..index(range.end.max(range.start))
    }
    fn replace(&mut self, range: Option<Range<usize>>, text: &str) -> usize {
        let range = range
            .or(self.marked.take())
            .unwrap_or(self.selection.clone());
        let before = self.bytes(range);
        let start = self.text[..before.start].encode_utf16().count();
        let value = text.replace(['\r', '\n'], " ");
        self.text.replace_range(before, &value);
        let end = start + value.encode_utf16().count();
        self.selection = end..end;
        start
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let end = self.text.encode_utf16().count();
        let byte = self.bytes(self.selection.clone());
        let left = self.selection.start.saturating_sub(
            self.text[..byte.start]
                .chars()
                .next_back()
                .map_or(0, char::len_utf16),
        );
        let right = (self.selection.end
            + self.text[byte.end..]
                .chars()
                .next()
                .map_or(0, char::len_utf16))
        .min(end);
        match (key.modifiers.control, key.key.as_str()) {
            (true, "a") => self.selection = 0..end,
            (true, "v") => {
                if let Some(text) = cx.read_from_clipboard().and_then(|v| v.text()) {
                    self.replace(None, &text);
                }
            }
            (true, "c") | (true, "x") => {
                if !byte.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.text[byte].into()));
                    if key.key == "x" {
                        self.replace(None, "");
                    }
                }
            }
            (false, "backspace") => {
                if self.selection.is_empty() {
                    self.selection.start = left;
                }
                self.replace(None, "");
            }
            (false, "delete") => {
                if self.selection.is_empty() {
                    self.selection.end = right;
                }
                self.replace(None, "");
            }
            (false, "left") => {
                let i = if self.selection.is_empty() {
                    left
                } else {
                    self.selection.start
                };
                self.selection = i..i;
            }
            (false, "right") => {
                let i = if self.selection.is_empty() {
                    right
                } else {
                    self.selection.end
                };
                self.selection = i..i;
            }
            (false, "home") => self.selection = 0..0,
            (false, "end") => self.selection = end..end,
            (false, "escape") => {
                if !self.text.is_empty() {
                    self.selection = 0..end;
                    self.replace(None, "");
                } else {
                    window.blur(cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }
}

impl EntityInputHandler for Search {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = self.bytes(range);
        *adjusted = Some(
            self.text[..bytes.start].encode_utf16().count()
                ..self.text[..bytes.end].encode_utf16().count(),
        );
        Some(self.text[bytes].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.selection.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone()
    }
    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace(range, text);
        self.marked = None;
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = self.replace(range, text);
        let count = text.encode_utf16().count();
        self.marked = (!text.is_empty()).then_some(start..start + count);
        if let Some(r) = selection {
            self.selection = start + r.start.min(count)..start + r.end.min(count);
        }
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for Search {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.clone();
        let entity = cx.entity();
        let palette = crate::theme::Palette::get(cx);
        let active = focus.is_focused(window);
        let byte = self.bytes(self.selection.clone());
        let mut body = div()
            .relative()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .h(px(32.))
            .px(px(10.))
            .rounded(px(10.))
            .bg(palette.fill())
            .border_1()
            .border_color(if active {
                palette.selection_border()
            } else {
                palette.rim()
            })
            .text_size(px(13.))
            .text_color(palette.ink())
            .cursor_text()
            .child(crate::glass::control_layer(palette, px(9.), true))
            .child(
                div().relative().flex().items_center().child(
                    svg()
                        .path("search")
                        .size(px(14.))
                        .text_color(palette.muted()),
                ),
            );
        if self.text.is_empty() && !active {
            body = body.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(palette.muted())
                    .child(self.placeholder.clone()),
            );
        } else {
            body = body.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .child(self.text[..byte.start].to_owned())
                    .when(active, |v| {
                        v.child(if byte.is_empty() {
                            div().w(px(1.)).h(px(15.)).bg(rgb(0x007aff))
                        } else {
                            div()
                                .bg(rgba(0x007aff41))
                                .child(self.text[byte.clone()].to_owned())
                        })
                    })
                    .when(!active, |v| v.child(self.text[byte.clone()].to_owned()))
                    .child(self.text[byte.end..].to_owned()),
            );
        }
        let has_text = !self.text.is_empty();
        body.child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                },
            )
            .absolute()
            .size_full(),
        )
        .when(has_text, |el| {
            el.child(
                div()
                    .id("search-clear-btn")
                    .relative()
                    .size(px(22.))
                    .flex_none()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(palette.fill_strong()))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path("x-small")
                            .size(px(10.))
                            .text_color(palette.muted()),
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.clear(cx);
                        this.focus.focus(window, cx);
                    })),
            )
        })
        .id("proxy-filter")
        .track_focus(&self.focus)
        .on_key_down(cx.listener(Self::key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.focus.focus(window, cx);
                cx.notify();
            }),
        )
    }
}
