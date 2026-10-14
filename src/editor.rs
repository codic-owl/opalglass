use crate::theme::MONO_FONT;
use gpui::{prelude::*, *};
use opalglass::buffer::Buffer;
use std::ops::Range;

const LINE: f32 = 25.;
const PAD: f32 = 20.;
const GUTTER: f32 = 64.;

struct Row {
    start: usize,
    top: Pixels,
    line: WrappedLine,
}

#[derive(Clone)]
struct SourceRow {
    range: Range<usize>,
    origin: Point<Pixels>,
    line: WrappedLine,
}
#[derive(Clone)]
struct CachedRow {
    range: Range<usize>,
    width: Pixels,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    line: WrappedLine,
}

pub struct Editor {
    pub buffer: Buffer,
    pub focus: FocusHandle,
    single_line: bool,
    placeholder: String,
    bounds: Bounds<Pixels>,
    rows: Vec<Row>,
    scroll: Pixels,
    scroll_x: Pixels,
    content_height: Pixels,
    reveal: bool,
    dragging: bool,
    preferred_x: Option<Pixels>,
    source_ranges: Vec<Range<usize>>,
    source_version: u64,
    source_list: ListState,
    source_width: Pixels,
    visible_rows: Vec<SourceRow>,
    cached_rows: Vec<CachedRow>,
    syntax_version: u64,
    pub source_links: Vec<Range<usize>>,
}

impl Editor {
    pub fn new(text: &str, single_line: bool, placeholder: &str, cx: &mut Context<Self>) -> Self {
        Self::from_buffer(
            Buffer::new(opalglass::document::normalize(text)),
            single_line,
            placeholder,
            cx,
        )
    }
    pub fn from_buffer(
        buffer: Buffer,
        single_line: bool,
        placeholder: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            buffer,
            focus: cx.focus_handle(),
            single_line,
            placeholder: placeholder.into(),
            bounds: Bounds::default(),
            rows: Vec::new(),
            scroll: px(0.),
            scroll_x: px(0.),
            content_height: px(0.),
            reveal: true,
            dragging: false,
            preferred_x: None,
            source_ranges: Vec::new(),
            source_version: u64::MAX,
            source_list: ListState::new(0, ListAlignment::Top, px(100.)),
            source_width: px(0.),
            visible_rows: Vec::new(),
            cached_rows: Vec::new(),
            syntax_version: u64::MAX,
            source_links: Vec::new(),
        }
    }
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.set_buffer(Buffer::new(opalglass::document::normalize(text)), cx);
    }
    pub fn set_buffer(&mut self, buffer: Buffer, cx: &mut Context<Self>) {
        self.buffer = buffer;
        self.preferred_x = None;
        self.source_version = u64::MAX;
        self.cached_rows.clear();
        self.source_links.clear();
        self.source_list.reset(0);
        self.scroll = px(0.);
        self.scroll_x = px(0.);
        self.reveal = true;
        cx.notify();
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.reveal = true;
        cx.notify();
    }
    pub fn jump_to(&mut self, index: usize, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.buffer.move_to(index, false);
        self.changed(cx);
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke.key;
        let modifiers = event.keystroke.modifiers;
        let command = (modifiers.control || modifiers.platform) && !modifiers.alt;
        let shift = modifiers.shift;
        if !matches!(key.as_str(), "up" | "down" | "pageup" | "pagedown") {
            self.preferred_x = None;
        }
        if command {
            match key.as_str() {
                "a" => self.buffer.select(0, self.buffer.text().len()),
                "c" => {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.selected().into()))
                }
                "x" => {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.selected().into()));
                    self.buffer.insert("");
                }
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        let text = self.clean_input(&text);
                        self.buffer.insert(&text);
                    }
                }
                "z" if shift => self.buffer.redo(),
                "z" => self.buffer.undo(),
                "y" => self.buffer.redo(),
                "left" => self.buffer.move_word(false, shift),
                "right" => self.buffer.move_word(true, shift),
                "backspace" => self.buffer.delete_word(true),
                "delete" => self.buffer.delete_word(false),
                "home" => self.buffer.move_to(0, shift),
                "end" => self.buffer.move_to(self.buffer.text().len(), shift),
                _ => return,
            }
        } else {
            match key.as_str() {
                "left" => self.buffer.horizontal(false, shift),
                "right" => self.buffer.horizontal(true, shift),
                "up" if !self.single_line => self.visual_move(false, shift, window, cx),
                "down" if !self.single_line => self.visual_move(true, shift, window, cx),
                "pageup" | "pagedown" if !self.single_line => {
                    let distance = (self.bounds.size.height - px(LINE)).max(px(LINE));
                    self.move_vertical(
                        if key == "pagedown" {
                            distance
                        } else {
                            -distance
                        },
                        shift,
                        window,
                        cx,
                    );
                }
                "home" => self
                    .buffer
                    .move_to(self.buffer.line_start(self.buffer.cursor()), shift),
                "end" => self
                    .buffer
                    .move_to(self.buffer.line_end(self.buffer.cursor()), shift),
                "backspace" => self.buffer.delete(true),
                "delete" => self.buffer.delete(false),
                "enter" if !self.single_line => self.buffer.insert("\n"),
                "tab" if !self.single_line => self.buffer.insert("    "),
                _ => return,
            }
        }
        self.focus.focus(window);
        cx.stop_propagation();
        self.changed(cx);
    }
    fn clean_input(&self, text: &str) -> String {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if self.single_line {
            text.replace(['\n', '\t'], " ")
        } else {
            text
        }
    }
    fn origin(&self) -> Point<Pixels> {
        self.bounds.origin
            + point(
                px(if self.single_line { 12. } else { PAD }) - self.scroll_x,
                px(if self.single_line { 7. } else { PAD }) - self.scroll,
            )
    }
    fn point_for(&self, byte: usize) -> Point<Pixels> {
        if !self.single_line {
            if let Some(row) = self
                .visible_rows
                .iter()
                .rev()
                .find(|r| r.range.start <= byte && byte <= r.range.end)
            {
                return row.origin
                    + row
                        .line
                        .position_for_index(byte - row.range.start, px(LINE))
                        .unwrap_or_default();
            }
            return self.bounds.origin;
        }
        for row in &self.rows {
            if byte <= row.start + row.line.len() {
                return self.origin()
                    + point(px(0.), row.top)
                    + row
                        .line
                        .position_for_index(byte.saturating_sub(row.start), px(LINE))
                        .unwrap_or_default();
            }
        }
        self.origin()
    }
    fn index_for(&self, position: Point<Pixels>) -> usize {
        if !self.single_line {
            if let Some(row) = self
                .visible_rows
                .iter()
                .find(|r| position.y < r.origin.y + r.line.size(px(LINE)).height)
                .or_else(|| self.visible_rows.last())
            {
                let local = point(
                    position.x - row.origin.x,
                    (position.y - row.origin.y).max(px(0.)),
                );
                return row.range.start
                    + row
                        .line
                        .closest_index_for_position(local, px(LINE))
                        .unwrap_or_else(|i| i)
                        .min(row.range.len());
            }
            return self.buffer.cursor();
        }
        let local = position - self.origin();
        for row in &self.rows {
            if local.y < row.top + row.line.size(px(LINE)).height {
                let index = row
                    .line
                    .closest_index_for_position(
                        point(local.x, (local.y - row.top).max(px(0.))),
                        px(LINE),
                    )
                    .unwrap_or_else(|i| i);
                return (row.start + index).min(self.buffer.text().len());
            }
        }
        self.buffer.text().len()
    }
    fn visual_move(
        &mut self,
        down: bool,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_vertical(px(if down { LINE } else { -LINE }), extend, window, cx);
    }
    fn move_vertical(
        &mut self,
        distance: Pixels,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_source();
        let cursor = self.buffer.cursor();
        let mut index = self
            .source_ranges
            .partition_point(|r| r.start <= cursor)
            .saturating_sub(1);
        let mut line = self.shaped_source_row(index, window, cx);
        let pos = line
            .position_for_index(cursor - self.source_ranges[index].start, px(LINE))
            .unwrap_or_default();
        let x = *self.preferred_x.get_or_insert(pos.x);
        let mut y = pos.y + px(LINE / 2.) + distance;
        loop {
            let height = line.size(px(LINE)).height.max(px(LINE));
            if y < px(0.) && index > 0 {
                index -= 1;
                line = self.shaped_source_row(index, window, cx);
                y += line.size(px(LINE)).height.max(px(LINE));
            } else if y >= height && index + 1 < self.source_ranges.len() {
                y -= height;
                index += 1;
                line = self.shaped_source_row(index, window, cx);
            } else {
                let range = &self.source_ranges[index];
                let offset = if y < px(0.) {
                    0
                } else if y >= height {
                    range.len()
                } else {
                    line.closest_index_for_position(point(x, y), px(LINE))
                        .unwrap_or_else(|i| i)
                };
                self.buffer
                    .move_to(range.start + offset.min(range.len()), extend);
                break;
            }
        }
    }
    fn shaped_source_row(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> WrappedLine {
        let _ = self.source_row(index, window, cx);
        self.cached_rows
            .iter()
            .rev()
            .find(|r| r.range == self.source_ranges[index])
            .unwrap()
            .line
            .clone()
    }
    pub fn set_source_links(&mut self, links: Vec<Range<usize>>) {
        self.syntax_version = self.buffer.revision();
        self.source_links = links;
        self.cached_rows.clear();
    }
    fn prepare_source(&mut self) {
        if self.single_line {
            return;
        }
        if self.source_version != self.buffer.revision() {
            if self.syntax_version != self.buffer.revision() {
                self.source_links.clear();
            }
            let offset = self.source_list.logical_scroll_top();
            self.source_ranges = opalglass::document::source_rows(self.buffer.text());
            self.source_list
                .splice(0..self.source_list.item_count(), self.source_ranges.len());
            self.source_list.scroll_to(offset);
            self.source_version = self.buffer.revision();
            self.cached_rows.clear();
        }
        if self.reveal {
            let index = self
                .source_ranges
                .partition_point(|r| r.start <= self.buffer.cursor())
                .saturating_sub(1);
            let visible = self.visible_rows.iter().any(|r| {
                r.range.start == self.source_ranges[index].start
                    && r.origin.y >= self.bounds.top()
                    && r.origin.y + px(LINE) <= self.bounds.bottom()
            });
            if visible {
                self.source_list.scroll_to_reveal_item(index);
            } else {
                self.source_list.scroll_to(ListOffset {
                    item_ix: index,
                    offset_in_item: px(0.),
                });
            }
            self.reveal = false;
        }
    }
    fn source_row(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let range = self.source_ranges[index].clone();
        let selection = self.buffer.selection();
        let selection = selection.start.clamp(range.start, range.end)
            ..selection.end.clamp(range.start, range.end);
        let marked =
            self.buffer.marked.as_ref().map(|r| {
                r.start.clamp(range.start, range.end)..r.end.clamp(range.start, range.end)
            });
        let width = self.source_width;
        let cached = self.cached_rows.iter().find(|r| {
            r.range == range && r.width == width && r.selection == selection && r.marked == marked
        });
        let line = if let Some(cached) = cached {
            cached.line.clone()
        } else {
            let text = &self.buffer.text()[range.clone()];
            let mut cuts = vec![range.start, range.end, selection.start, selection.end];
            let first_link = self.source_links.partition_point(|r| r.end <= range.start);
            let links: Vec<_> = self.source_links[first_link..]
                .iter()
                .take_while(|r| r.start < range.end)
                .collect();
            for r in &links {
                cuts.extend([r.start.max(range.start), r.end.min(range.end)]);
            }
            if let Some(r) = &marked {
                cuts.extend([r.start, r.end]);
            }
            cuts.sort_unstable();
            cuts.dedup();
            let line_start = self.buffer.line_start(range.start);
            let prefix = self.buffer.text().as_bytes().get(line_start);
            let runs: Vec<_> = cuts
                .windows(2)
                .map(|pair| TextRun {
                    len: pair[1] - pair[0],
                    font: font(MONO_FONT),
                    color: rgb(
                        if prefix == Some(&b'#') || links.iter().any(|r| r.contains(&pair[0])) {
                            0xb8a5fa
                        } else if prefix == Some(&b'>') {
                            0x9bbfb3
                        } else {
                            0xd6d9e3
                        },
                    )
                    .into(),
                    background_color: (pair[0] >= selection.start && pair[1] <= selection.end)
                        .then(|| rgb(0x38496a).into()),
                    underline: marked
                        .as_ref()
                        .filter(|r| r.start <= pair[0] && pair[1] <= r.end)
                        .map(|_| UnderlineStyle {
                            thickness: px(1.),
                            color: Some(rgb(0xb6a1ff).into()),
                            wavy: false,
                        }),
                    strikethrough: None,
                })
                .collect();
            let line = window
                .text_system()
                .shape_text(text.to_owned().into(), px(14.), &runs, Some(width), None)
                .unwrap_or_default()
                .into_iter()
                .next()
                .unwrap_or_default();
            if self.cached_rows.len() >= 256 {
                self.cached_rows.drain(..128);
            }
            self.cached_rows.push(CachedRow {
                range: range.clone(),
                width,
                selection,
                marked,
                line: line.clone(),
            });
            line
        };
        let height = line.size(px(LINE)).height.max(px(LINE));
        let entity = cx.entity();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                entity.update(cx, |editor, cx| {
                    let origin = bounds.origin + point(px(GUTTER), px(0.));
                    let active = editor.buffer.line_start(editor.buffer.cursor())
                        == editor.buffer.line_start(range.start);
                    if active
                        && editor.focus.is_focused(window)
                        && editor.buffer.selection().is_empty()
                    {
                        window.paint_quad(fill(bounds, rgb(0x1d2230)));
                    }
                    if editor.buffer.line_start(range.start) == range.start {
                        let number = editor.buffer.line_number(range.start).to_string();
                        let number_line = window.text_system().shape_line(
                            number.clone().into(),
                            px(12.),
                            &[TextRun {
                                len: number.len(),
                                font: font(MONO_FONT),
                                color: rgb(if active { 0xb8a5fa } else { 0x7d879c }).into(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            None,
                        );
                        let number_origin =
                            bounds.origin + point(px(GUTTER - 16.) - number_line.width, px(0.));
                        let _ = number_line.paint(number_origin, px(LINE), window, cx);
                    }
                    editor.visible_rows.push(SourceRow {
                        range: range.clone(),
                        origin,
                        line: line.clone(),
                    });
                    let _ =
                        line.paint_background(origin, px(LINE), TextAlign::Left, None, window, cx);
                    let _ = line.paint(origin, px(LINE), TextAlign::Left, None, window, cx);
                    if editor.buffer.text().is_empty() && !editor.placeholder.is_empty() {
                        let placeholder = window.text_system().shape_line(
                            editor.placeholder.clone().into(),
                            px(14.),
                            &[TextRun {
                                len: editor.placeholder.len(),
                                font: font(MONO_FONT),
                                color: rgb(0x687187).into(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            None,
                        );
                        let _ = placeholder.paint(origin, px(LINE), window, cx);
                    }
                    let cursor = editor.buffer.cursor();
                    let cursor_row = editor
                        .source_ranges
                        .partition_point(|r| r.start <= cursor)
                        .saturating_sub(1);
                    if editor.focus.is_focused(window) && cursor_row == index {
                        let pos = origin
                            + line
                                .position_for_index(cursor - range.start, px(LINE))
                                .unwrap_or_default();
                        window.paint_quad(fill(
                            Bounds::new(pos + point(px(0.), px(3.)), size(px(2.), px(LINE - 5.))),
                            rgb(0xb7a5ff),
                        ));
                    }
                });
            },
        )
        .w_full()
        .h(height)
        .into_any_element()
    }
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.bounds = bounds;
        let text = self.buffer.text();
        let selected = self.buffer.selection();
        let mut cuts = vec![0, selected.start, selected.end, text.len()];
        if let Some(range) = &self.buffer.marked {
            cuts.extend([range.start, range.end]);
        }
        cuts.sort_unstable();
        cuts.dedup();
        let runs: Vec<_> = cuts
            .windows(2)
            .filter(|pair| pair[0] < pair[1])
            .map(|pair| TextRun {
                len: pair[1] - pair[0],
                font: font(MONO_FONT),
                color: rgb(0xd6d9e3).into(),
                background_color: (pair[0] >= selected.start && pair[1] <= selected.end)
                    .then(|| rgb(0x38496a).into()),
                underline: self
                    .buffer
                    .marked
                    .as_ref()
                    .filter(|r| pair[0] >= r.start && pair[1] <= r.end)
                    .map(|_| UnderlineStyle {
                        thickness: px(1.),
                        color: Some(rgb(0xb6a1ff).into()),
                        wavy: false,
                    }),
                strikethrough: None,
            })
            .collect();
        let width =
            (bounds.size.width - px(if self.single_line { 24. } else { PAD * 2. })).max(px(20.));
        let shaped = window
            .text_system()
            .shape_text(
                text.to_owned().into(),
                px(14.),
                &runs,
                (!self.single_line).then_some(width),
                None,
            )
            .unwrap_or_default();
        let mut start = 0;
        let mut top = px(0.);
        self.rows = shaped
            .into_iter()
            .map(|line| {
                let row = Row { start, top, line };
                start += row.line.len() + 1;
                top += row.line.size(px(LINE)).height;
                row
            })
            .collect();
        self.content_height = top + px(PAD * 2.);
        if self.reveal {
            let caret = self.point_for(self.buffer.cursor()) - bounds.origin;
            if self.single_line {
                if caret.x < px(12.) {
                    self.scroll_x = (self.scroll_x + caret.x - px(12.)).max(px(0.));
                } else if caret.x + px(14.) > bounds.size.width {
                    self.scroll_x += caret.x + px(14.) - bounds.size.width;
                }
            } else if caret.y < px(8.) {
                self.scroll = (self.scroll + caret.y - px(8.)).max(px(0.));
            } else if caret.y + px(LINE + 8.) > bounds.size.height {
                self.scroll += caret.y + px(LINE + 8.) - bounds.size.height;
            }
            self.reveal = false;
        }
        self.scroll = self.scroll.clamp(
            px(0.),
            (self.content_height - bounds.size.height).max(px(0.)),
        );
        let origin = self.origin();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if self.buffer.text().is_empty() && !self.placeholder.is_empty() {
                let run = TextRun {
                    len: self.placeholder.len(),
                    font: font(MONO_FONT),
                    color: rgb(0x687187).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window.text_system().shape_line(
                    self.placeholder.clone().into(),
                    px(14.),
                    &[run],
                    None,
                );
                let _ = line.paint(origin, px(LINE), window, cx);
            }
            for row in &self.rows {
                let pos = origin + point(px(0.), row.top);
                if pos.y + row.line.size(px(LINE)).height < bounds.origin.y
                    || pos.y > bounds.bottom()
                {
                    continue;
                }
                let _ = row
                    .line
                    .paint_background(pos, px(LINE), TextAlign::Left, None, window, cx);
                let _ = row
                    .line
                    .paint(pos, px(LINE), TextAlign::Left, None, window, cx);
            }
            if self.focus.is_focused(window) {
                let pos = self.point_for(self.buffer.cursor());
                window.paint_quad(fill(
                    Bounds::new(pos + point(px(0.), px(3.)), size(px(2.), px(LINE - 5.))),
                    rgb(0xb7a5ff),
                ));
            }
            if self.content_height > bounds.size.height && !self.single_line {
                let height =
                    (bounds.size.height * (bounds.size.height / self.content_height)).max(px(24.));
                let y = (bounds.size.height - height)
                    * (self.scroll / (self.content_height - bounds.size.height));
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.right() - px(5.), bounds.top() + y),
                        size(px(3.), height),
                    ),
                    rgb(0x414b61),
                ));
            }
        });
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, cx.entity()),
            cx,
        );
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.prepare_source();
        let entity = cx.entity();
        let content = if self.single_line {
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    entity.update(cx, |editor, cx| editor.paint(bounds, window, cx));
                },
            )
            .size_full()
            .into_any_element()
        } else {
            let prepaint_entity = entity.clone();
            let input_entity = entity.clone();
            div()
                .size_full()
                .relative()
                .child(
                    canvas(
                        move |bounds, window, cx| {
                            prepaint_entity.update(cx, |editor, _| {
                                editor.bounds = bounds;
                                editor.visible_rows.clear();
                                let width =
                                    (bounds.size.width - px(PAD * 2. + GUTTER)).max(px(20.));
                                if editor.source_width != width {
                                    editor.source_width = width;
                                    editor.cached_rows.clear();
                                    let offset = editor.source_list.logical_scroll_top();
                                    editor.source_list.splice(
                                        0..editor.source_list.item_count(),
                                        editor.source_ranges.len(),
                                    );
                                    editor.source_list.scroll_to(offset);
                                }
                                let _ = window;
                            });
                        },
                        move |bounds, _, window, cx| {
                            input_entity.update(cx, |editor, cx| {
                                window.handle_input(
                                    &editor.focus,
                                    ElementInputHandler::new(bounds, cx.entity()),
                                    cx,
                                );
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    list(
                        self.source_list.clone(),
                        cx.processor(|this, index, window, cx| this.source_row(index, window, cx)),
                    )
                    .size_full()
                    .px(px(PAD))
                    .py(px(PAD)),
                )
                .into_any_element()
        };
        div()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window);
                    this.preferred_x = None;
                    let index = this.index_for(event.position);
                    this.buffer.move_to(index, event.modifiers.shift);
                    if event.click_count >= 3 {
                        this.buffer
                            .select(this.buffer.line_start(index), this.buffer.line_end(index));
                    } else if event.click_count == 2 {
                        let text = this.buffer.text();
                        let start = text[..index]
                            .char_indices()
                            .rev()
                            .find(|(_, c)| c.is_whitespace())
                            .map_or(0, |(i, c)| i + c.len_utf8());
                        let end = text[index..]
                            .find(char::is_whitespace)
                            .map_or(text.len(), |i| index + i);
                        this.buffer.select(start, end);
                    }
                    this.dragging = true;
                    cx.stop_propagation();
                    this.changed(cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.dragging && event.pressed_button == Some(MouseButton::Left) {
                    this.buffer.move_to(this.index_for(event.position), true);
                    this.changed(cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .child(content)
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = self.buffer.from_utf16(range);
        *adjusted = Some(self.buffer.to_utf16(bytes.clone()));
        self.buffer.text().get(bytes).map(str::to_owned)
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.to_utf16(self.buffer.selection()),
            reversed: self.buffer.cursor() < self.buffer.anchor(),
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer.marked.clone().map(|r| self.buffer.to_utf16(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.finish_composition();
        self.changed(cx);
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| self.buffer.from_utf16(r))
            .or_else(|| self.buffer.marked.clone())
            .unwrap_or_else(|| self.buffer.selection());
        let text = self.clean_input(text);
        self.preferred_x = None;
        self.buffer.replace(range, &text);
        self.changed(cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|r| self.buffer.from_utf16(r));
        let text = self.clean_input(text);
        self.preferred_x = None;
        self.buffer.compose(range, &text);
        if let Some(selected) = selected {
            let start = self
                .buffer
                .byte_to_utf16(self.buffer.marked.as_ref().unwrap().start);
            let selected = self
                .buffer
                .from_utf16(start + selected.start..start + selected.end);
            self.buffer.select(selected.start, selected.end);
        }
        self.changed(cx);
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let byte = self.buffer.utf16_to_byte(range.start);
        if !self.single_line
            && !self
                .visible_rows
                .iter()
                .any(|r| r.range.start <= byte && byte <= r.range.end)
        {
            return None;
        }
        Some(Bounds::new(self.point_for(byte), size(px(2.), px(LINE))))
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.buffer.byte_to_utf16(self.index_for(point)))
    }
}
