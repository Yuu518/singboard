use gpui::{prelude::*, *};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

struct LayoutState {
    line: Option<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
}

pub struct Search {
    pub text: String,
    pub placeholder: SharedString,
    pub focus: FocusHandle,
    selection: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    dragging: bool,
    layout: Rc<RefCell<LayoutState>>,
}

impl Search {
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.text.clear();
        self.selection = 0..0;
        self.reversed = false;
        self.marked = None;
        self.dragging = false;
        self.layout.borrow_mut().scroll_x = px(0.);
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
        self.reversed = false;
        cx.notify();
    }
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            placeholder: "搜索（Ctrl+F）".into(),
            focus: cx.focus_handle(),
            selection: 0..0,
            reversed: false,
            marked: None,
            dragging: false,
            layout: Rc::new(RefCell::new(LayoutState {
                line: None,
                bounds: None,
                scroll_x: px(0.),
            })),
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
    fn utf16_index_from_byte(&self, byte_idx: usize) -> usize {
        let clamped = byte_idx.min(self.text.len());
        let mut boundary = clamped;
        while boundary > 0 && !self.text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        self.text[..boundary].encode_utf16().count()
    }
    fn step_left(&self, utf16: usize) -> usize {
        let byte = self.bytes(utf16..utf16).start;
        utf16.saturating_sub(
            self.text[..byte]
                .chars()
                .next_back()
                .map_or(0, char::len_utf16),
        )
    }
    fn step_right(&self, utf16: usize) -> usize {
        let end = self.text.encode_utf16().count();
        let byte = self.bytes(utf16..utf16).start;
        (utf16
            + self.text[byte..]
                .chars()
                .next()
                .map_or(0, char::len_utf16))
        .min(end)
    }
    fn index_for_point(&self, point: Point<Pixels>) -> Option<usize> {
        let state = self.layout.borrow();
        let (line, bounds) = (state.line.as_ref()?, state.bounds?);
        if self.text.is_empty() {
            return Some(0);
        }
        let x = (point.x - bounds.origin.x + state.scroll_x).max(px(0.));
        let byte_idx = line.closest_index_for_x(x);
        Some(self.utf16_index_from_byte(byte_idx))
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
        self.reversed = false;
        start
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if self.marked.is_some()
            && !key.modifiers.control
            && matches!(
                key.key.as_str(),
                "left" | "right" | "home" | "end" | "backspace" | "delete" | "enter" | "escape"
            )
        {
            return;
        }
        let end = self.text.encode_utf16().count();
        let byte = self.bytes(self.selection.clone());
        let head = if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        };
        let anchor = if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        };
        let shift = key.modifiers.shift;
        match (key.modifiers.control, key.key.as_str()) {
            (true, "a") => {
                self.selection = 0..end;
                self.reversed = false;
            }
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
                    self.selection.start = self.step_left(self.selection.start);
                }
                self.replace(None, "");
            }
            (false, "delete") => {
                if self.selection.is_empty() {
                    self.selection.end = self.step_right(self.selection.end);
                }
                self.replace(None, "");
            }
            (false, "left") => {
                if shift {
                    let next = self.step_left(head);
                    self.selection = anchor.min(next)..anchor.max(next);
                    self.reversed = next < anchor;
                } else {
                    let i = if self.selection.is_empty() {
                        self.step_left(self.selection.start)
                    } else {
                        self.selection.start
                    };
                    self.selection = i..i;
                    self.reversed = false;
                }
            }
            (false, "right") => {
                if shift {
                    let next = self.step_right(head);
                    self.selection = anchor.min(next)..anchor.max(next);
                    self.reversed = next < anchor;
                } else {
                    let i = if self.selection.is_empty() {
                        self.step_right(self.selection.end)
                    } else {
                        self.selection.end
                    };
                    self.selection = i..i;
                    self.reversed = false;
                }
            }
            (false, "home") => {
                if shift {
                    self.selection = 0..anchor;
                    self.reversed = anchor > 0;
                } else {
                    self.selection = 0..0;
                    self.reversed = false;
                }
            }
            (false, "end") => {
                if shift {
                    self.selection = anchor..end;
                    self.reversed = false;
                } else {
                    self.selection = end..end;
                    self.reversed = false;
                }
            }
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
            reversed: self.reversed,
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
            self.reversed = false;
        }
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let state = self.layout.borrow();
        let (Some(line), Some(text_bounds)) = (state.line.as_ref(), state.bounds) else {
            return Some(bounds);
        };
        let bytes = self.bytes(range);
        let start_x = line.x_for_index(bytes.start) - state.scroll_x;
        let end_x = line.x_for_index(bytes.end) - state.scroll_x;
        Some(Bounds::new(
            point(text_bounds.origin.x + start_x, text_bounds.origin.y),
            size((end_x - start_x).max(px(1.)), text_bounds.size.height),
        ))
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        self.index_for_point(point)
    }
}

impl Render for Search {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.clone();
        let entity = cx.entity();
        let palette = crate::theme::Palette::get(cx);
        let active = focus.is_focused(window);
        let byte = self.bytes(self.selection.clone());
        let reversed = self.reversed;
        let text_value = self.text.clone();
        let placeholder_value = self.placeholder.clone();
        let ink = palette.ink();
        let muted = palette.muted();
        let layout_state = self.layout.clone();
        let has_text = !self.text.is_empty();

        div()
            .relative()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .h(crate::theme::text_px(32.))
            .px(px(10.))
            .rounded(px(10.))
            .bg(palette.fill())
            .border_1()
            .border_color(if active {
                palette.selection_border()
            } else {
                palette.control_rim()
            })
            .text_sm()
            .text_color(ink)
            .cursor_text()
            .child(crate::glass::control_layer(palette, px(9.), true))
            .child(
                div().relative().flex().items_center().child(
                    svg()
                        .path("search")
                        .size(px(14.))
                        .text_color(muted),
                ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, cx| {
                                window.handle_input(
                                    &focus,
                                    ElementInputHandler::new(bounds, entity.clone()),
                                    cx,
                                );
                                let style = window.text_style();
                                let font_size = style.font_size.to_pixels(window.rem_size());
                                let line_height = window.line_height();
                                let show_placeholder = text_value.is_empty() && !active;
                                let display: SharedString = if show_placeholder {
                                    placeholder_value.clone()
                                } else {
                                    text_value.clone().into()
                                };
                                let color = if show_placeholder { muted } else { ink };
                                let run = TextRun {
                                    len: display.len(),
                                    font: style.font(),
                                    color: color.into(),
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                };
                                let line = window.text_system().shape_line(
                                    display,
                                    font_size,
                                    &[run],
                                    None,
                                );
                                let mut scroll_x = layout_state.borrow().scroll_x;
                                if text_value.is_empty() {
                                    scroll_x = px(0.);
                                } else {
                                    let caret_byte = if reversed { byte.start } else { byte.end };
                                    let caret_x = line.x_for_index(caret_byte);
                                    let avail = (bounds.size.width - px(2.)).max(px(1.));
                                    if caret_x - scroll_x > avail {
                                        scroll_x = caret_x - avail;
                                    } else if caret_x < scroll_x {
                                        scroll_x = caret_x;
                                    }
                                    let max_scroll = (line.width - avail).max(px(0.));
                                    scroll_x = scroll_x.clamp(px(0.), max_scroll);
                                }
                                {
                                    let mut state = layout_state.borrow_mut();
                                    state.line = if show_placeholder { None } else { Some(line.clone()) };
                                    state.bounds = Some(bounds);
                                    state.scroll_x = scroll_x;
                                }
                                let text_y = bounds.origin.y
                                    + ((bounds.size.height - line_height) * 0.5).max(px(0.));
                                let text_origin = point(bounds.origin.x - scroll_x, text_y);
                                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                    if active && !byte.is_empty() && !text_value.is_empty() {
                                        let sel_start = line.x_for_index(byte.start);
                                        let sel_end = line.x_for_index(byte.end);
                                        let sel_bounds = Bounds::new(
                                            point(bounds.origin.x + sel_start - scroll_x, text_y),
                                            size(
                                                (sel_end - sel_start).max(px(1.)),
                                                line_height.min(bounds.size.height),
                                            ),
                                        );
                                        window.paint_quad(fill(sel_bounds, rgba(0x007aff41)));
                                    }
                                    let _ = line.paint(
                                        text_origin,
                                        line_height,
                                        TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    );
                                    if active && byte.is_empty() {
                                        let caret_x = if text_value.is_empty() {
                                            px(0.)
                                        } else {
                                            line.x_for_index(byte.start)
                                        };
                                        let caret_h = px(15.).min(bounds.size.height);
                                        let caret_y = bounds.origin.y
                                            + ((bounds.size.height - caret_h) * 0.5).max(px(0.));
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                point(
                                                    bounds.origin.x + caret_x - scroll_x,
                                                    caret_y,
                                                ),
                                                size(px(1.5), caret_h),
                                            ),
                                            rgb(0x007aff),
                                        ));
                                    }
                                });
                            },
                        )
                        .size_full(),
                    ),
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
                                .text_color(muted),
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
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    if event.click_count >= 2 {
                        this.selection = 0..this.text.encode_utf16().count();
                        this.reversed = false;
                        this.dragging = false;
                    } else if let Some(idx) = this.index_for_point(event.position) {
                        if event.modifiers.shift {
                            let anchor = if this.reversed {
                                this.selection.end
                            } else {
                                this.selection.start
                            };
                            this.selection = anchor.min(idx)..anchor.max(idx);
                            this.reversed = idx < anchor;
                        } else {
                            this.selection = idx..idx;
                            this.reversed = false;
                        }
                        this.dragging = true;
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if !this.dragging || !event.dragging() {
                    return;
                }
                if let Some(idx) = this.index_for_point(event.position) {
                    let anchor = if this.reversed {
                        this.selection.end
                    } else {
                        this.selection.start
                    };
                    this.selection = anchor.min(idx)..anchor.max(idx);
                    this.reversed = idx < anchor;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.dragging = false;
                }),
            )
    }
}
