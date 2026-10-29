use gpui::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, ShapedLine, TextRun, UTF16Selection, Window, canvas, div,
    fill, point, prelude::*, px, rgb, size,
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub fn bind_keys(_cx: &mut App) {}

pub struct TextChanged;
impl EventEmitter<TextChanged> for TextInput {}

#[derive(Clone, Copy, PartialEq)]
pub struct EditorStyle {
    pub font_size: f32,
    pub color: u32,
    pub centered: bool,
    pub wrap: bool,
}
impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            font_size: 15.0,
            color: 0xfafafa,
            centered: false,
            wrap: false,
        }
    }
}

pub struct TextInput {
    pub focus: FocusHandle,
    pub content: String,
    selection: Range<usize>,
    anchor: usize,
    marked: Option<Range<usize>>,
    layout: Vec<Row>,
    bounds: Option<Bounds<Pixels>>,
    scroll_x: f32,
    scroll_y: f32,
    pub style: EditorStyle,
    selecting: bool,
}
struct Row {
    start: usize,
    end: usize,
    line: ShapedLine,
    origin: Point<Pixels>,
    paragraph_end: bool,
}
impl TextInput {
    pub fn new(content: String, cx: &mut Context<Self>) -> Self {
        let end = content.len();
        Self {
            focus: cx.focus_handle(),
            content,
            selection: end..end,
            anchor: end,
            marked: None,
            layout: Vec::new(),
            bounds: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            style: EditorStyle::default(),
            selecting: false,
        }
    }
    fn cursor(&self) -> usize {
        if self.selection.start == self.anchor {
            self.selection.end
        } else {
            self.selection.start
        }
    }
    fn select(&mut self, end: usize, extend: bool, cx: &mut Context<Self>) {
        if !extend {
            self.anchor = end;
        }
        self.selection = self.anchor.min(end)..self.anchor.max(end);
        cx.notify();
    }
    fn previous(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }
    fn next(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.content.len())
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let m = event.keystroke.modifiers;
        let command = m.control || m.platform;
        match (key, command) {
            ("a", true) => {
                self.anchor = 0;
                self.selection = 0..self.content.len();
            }
            ("c" | "x", true) => {
                if !self.selection.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        self.content[self.selection.clone()].into(),
                    ));
                    if key == "x" {
                        self.replace_text_in_range(None, "", window, cx);
                    }
                }
            }
            ("v", true) => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.replace_text_in_range(None, &text, window, cx);
                }
            }
            ("backspace", false) => {
                if self.selection.is_empty() {
                    self.selection.start = self.previous(self.selection.start);
                }
                self.replace_text_in_range(None, "", window, cx);
            }
            ("delete", false) => {
                if self.selection.is_empty() {
                    self.selection.end = self.next(self.selection.end);
                }
                self.replace_text_in_range(None, "", window, cx);
            }
            ("left", false) => {
                let offset = if !m.shift && !self.selection.is_empty() {
                    self.selection.start
                } else {
                    self.previous(self.cursor())
                };
                self.select(offset, m.shift, cx);
            }
            ("right", false) => {
                let offset = if !m.shift && !self.selection.is_empty() {
                    self.selection.end
                } else {
                    self.next(self.cursor())
                };
                self.select(offset, m.shift, cx);
            }
            ("home", _) => {
                let cursor = self.cursor();
                let offset = if command {
                    0
                } else {
                    self.content[..cursor].rfind('\n').map_or(0, |i| i + 1)
                };
                self.select(offset, m.shift, cx);
            }
            ("end", _) => {
                let cursor = self.cursor();
                let offset = if command {
                    self.content.len()
                } else {
                    self.content[cursor..]
                        .find('\n')
                        .map_or(self.content.len(), |i| cursor + i)
                };
                self.select(offset, m.shift, cx);
            }
            ("up" | "down", false) => {
                let current = self
                    .layout
                    .iter()
                    .position(|row| {
                        row.start <= self.cursor() && (row.end > self.cursor() || row.paragraph_end)
                    })
                    .unwrap_or(0);
                let target = if key == "up" {
                    current.saturating_sub(1)
                } else {
                    (current + 1).min(self.layout.len().saturating_sub(1))
                };
                if let (Some(from), Some(to)) = (self.layout.get(current), self.layout.get(target))
                {
                    let x = from.origin.x
                        + from
                            .line
                            .x_for_index(self.cursor().saturating_sub(from.start));
                    let offset = to.start + to.line.closest_index_for_x(x - to.origin.x);
                    self.select(offset, m.shift, cx);
                }
            }
            ("enter", false) => self.replace_text_in_range(None, "\n", window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn click(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.selecting = true;
        let offset = self.index_at(event.position);
        self.select(offset, event.modifiers.shift, cx);
        cx.stop_propagation();
    }
    fn motion(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select(self.index_at(event.position), true, cx);
        }
    }
    fn release(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }
    fn index_at(&self, p: Point<Pixels>) -> usize {
        let Some(first) = self.layout.first() else {
            return self.content.len();
        };
        let index = ((p.y - first.origin.y) / px(self.style.font_size * 1.35)).max(0.0) as usize;
        self.layout
            .get(index.min(self.layout.len() - 1))
            .map_or(self.content.len(), |row| {
                row.start + row.line.closest_index_for_x(p.x - row.origin.x)
            })
    }
    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut count = 0;
        for (i, c) in self.content.char_indices() {
            if count >= offset {
                return i;
            }
            count += c.len_utf16();
        }
        self.content.len()
    }
    fn to_utf16(&self, offset: usize) -> usize {
        self.content[..offset.min(self.content.len())]
            .encode_utf16()
            .count()
    }
    fn utf8_range(&self, r: Range<usize>) -> Range<usize> {
        self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end)
    }
    fn utf16_range(&self, r: Range<usize>) -> Range<usize> {
        self.to_utf16(r.start)..self.to_utf16(r.end)
    }
    fn draw(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.bounds = Some(bounds);
        self.layout.clear();
        let font = self.style.font_size;
        let height = font * 1.35;
        let mut offset = 0;
        for part in self.content.split('\n') {
            let run = TextRun {
                len: part.len(),
                font: gpui::font("Inter Variable"),
                color: rgb(self.style.color).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let mut breaks = vec![0];
            if self.style.wrap
                && !part.is_empty()
                && let Ok(lines) = window.text_system().shape_text(
                    part.to_owned().into(),
                    px(font),
                    std::slice::from_ref(&run),
                    Some(bounds.size.width),
                    None,
                )
            {
                for line in lines {
                    for boundary in line.wrap_boundaries() {
                        breaks.push(line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index);
                    }
                }
            }
            breaks.push(part.len());
            let last = breaks.len() - 2;
            for (i, pair) in breaks.windows(2).enumerate() {
                let segment = &part[pair[0]..pair[1]];
                let line = window.text_system().shape_line(
                    segment.to_owned().into(),
                    px(font),
                    &[TextRun {
                        len: segment.len(),
                        ..run.clone()
                    }],
                    None,
                );
                self.layout.push(Row {
                    start: offset + pair[0],
                    end: offset + pair[1],
                    line,
                    origin: point(px(0.0), px(0.0)),
                    paragraph_end: i == last,
                });
            }
            offset += part.len() + 1;
        }
        let cursor = self.cursor();
        let cursor_row = self
            .layout
            .iter()
            .position(|r| r.start <= cursor && (cursor < r.end || r.paragraph_end))
            .unwrap_or(0);
        let cursor_x = self
            .layout
            .get(cursor_row)
            .map_or(0.0, |r| f32::from(r.line.x_for_index(cursor - r.start)));
        let total = self.layout.len() as f32 * height;
        let inset_y = if self.style.centered {
            ((f32::from(bounds.size.height) - total) * 0.5).max(0.0)
        } else {
            0.0
        };
        self.scroll_y =
            (cursor_row as f32 * height + height - f32::from(bounds.size.height)).max(0.0);
        self.scroll_x = if self.style.wrap {
            0.0
        } else {
            (cursor_x + 4.0 - f32::from(bounds.size.width)).max(0.0)
        };
        let text_color = self.style.color;
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for (i, row) in self.layout.iter_mut().enumerate() {
                let x = if self.style.centered {
                    ((f32::from(bounds.size.width - row.line.width)) * 0.5).max(0.0)
                } else {
                    0.0
                };
                row.origin = bounds.origin
                    + point(
                        px(x - self.scroll_x),
                        px(inset_y + i as f32 * height - self.scroll_y),
                    );
                let start = self.selection.start.max(row.start).min(row.end) - row.start;
                let end = self.selection.end.max(row.start).min(row.end) - row.start;
                if end > start {
                    let mut color = rgb(text_color);
                    color.a = 0.2;
                    window.paint_quad(fill(
                        Bounds::new(
                            row.origin + point(row.line.x_for_index(start), px(0.0)),
                            size(
                                row.line.x_for_index(end) - row.line.x_for_index(start),
                                px(height),
                            ),
                        ),
                        color,
                    ));
                }
                let _ = row.line.paint(row.origin, px(height), window, cx);
                if i == cursor_row && self.focus.is_focused(window) {
                    window.paint_quad(fill(
                        Bounds::new(
                            row.origin + point(px(cursor_x), px(font * 0.15)),
                            size(px(1.5), px(font)),
                        ),
                        rgb(text_color),
                    ));
                }
            }
        });
    }
}
impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.utf8_range(range);
        *actual = Some(self.utf16_range(r.clone()));
        Some(self.content[r].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.utf16_range(self.selection.clone()),
            reversed: self.cursor() == self.selection.start,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone().map(|r| self.utf16_range(r))
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
        let r = range
            .map(|r| self.utf8_range(r))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        if self.content.len() - r.len() + text.len() > 16_000 {
            return;
        }
        self.content.replace_range(r.clone(), text);
        self.anchor = r.start + text.len();
        self.selection = self.anchor..self.anchor;
        self.marked = None;
        cx.emit(TextChanged);
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = range
            .clone()
            .map(|r| self.utf8_range(r))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        if self.content.len() - r.len() + text.len() > 16_000 {
            return;
        }
        self.replace_text_in_range(range, text, window, cx);
        self.marked = Some(r.start..r.start + text.len());
        if let Some(s) = selection {
            let mut count = 0;
            let mut start = text.len();
            let mut end = text.len();
            for (i, c) in text.char_indices() {
                if count >= s.start && start == text.len() {
                    start = i;
                }
                if count >= s.end {
                    end = i;
                    break;
                }
                count += c.len_utf16();
            }
            self.selection = r.start + start..r.start + end;
            self.anchor = self.selection.start;
        }
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let r = self.utf8_range(range);
        let row = self
            .layout
            .iter()
            .find(|row| row.start <= r.start && (row.end > r.start || row.paragraph_end))?;
        Some(Bounds::new(
            row.origin + point(row.line.x_for_index(r.start - row.start), px(0.0)),
            size(px(1.0), px(self.style.font_size * 1.35)),
        ))
    }
    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.to_utf16(self.index_at(p)))
    }
}
impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        div()
            .size_full()
            .track_focus(&self.focus)
            .key_context("TextInput")
            .cursor(gpui::CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::click))
            .on_mouse_move(cx.listener(Self::motion))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::release))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::release))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        let focus = entity.read(cx).focus.clone();
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, entity.clone()),
                            cx,
                        );
                        entity.update(cx, |input, cx| input.draw(bounds, window, cx));
                    },
                )
                .size_full(),
            )
    }
}
