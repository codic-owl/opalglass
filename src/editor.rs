use crate::theme::{MISSING_LINK, MONO_FONT, UI_FONT, missing_link_underline};
use gpui::{prelude::*, *};
use opalglass::{buffer::Buffer, live};
use std::{ops::Range, sync::Arc};

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
    projection: Arc<live::Line>,
    line_height: Pixels,
}
#[derive(Clone)]
struct CachedRow {
    range: Range<usize>,
    width: Pixels,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    line: WrappedLine,
    projection: Arc<live::Line>,
    line_height: Pixels,
    revealed: bool,
}

pub struct ScrollAnchor;
pub struct FollowLink(pub String);
pub struct PasteImages(pub Vec<Image>);
impl EventEmitter<PasteImages> for Editor {}
impl EventEmitter<FollowLink> for Editor {}
#[derive(Clone, Copy)]
pub enum CompletionAction {
    Previous,
    Next,
    Accept,
    Dismiss,
}
impl EventEmitter<CompletionAction> for Editor {}

impl EventEmitter<ScrollAnchor> for Editor {}

pub struct Editor {
    #[cfg(test)]
    pub(crate) render_count: usize,
    #[cfg(test)]
    pub(crate) first_paint_at: Option<std::time::Instant>,
    #[cfg(test)]
    pub(crate) first_paint_indexed_rows: Option<usize>,
    pub buffer: Buffer,
    pub focus: FocusHandle,
    single_line: bool,
    pub live_preview: bool,
    pub completion_visible: bool,
    pub accepts_attachments: bool,
    pub font_size: f32,
    pub line_numbers: bool,
    live_revealed: Option<Range<usize>>,
    live_lines: Vec<(Range<usize>, bool, Arc<live::Line>)>,
    code_blocks: Vec<Range<usize>>,
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
    source_complete: bool,
    source_requested_revision: u64,
    _source_prepare: Option<Task<()>>,
    source_list: ListState,
    pending_scroll: Option<opalglass::navigation::ScrollPosition>,
    pub scroll_anchor: Option<(u64, f64)>,
    source_width: Pixels,
    visible_rows: Vec<SourceRow>,
    cached_rows: Vec<CachedRow>,
    syntax_version: u64,
    pub source_links: Vec<Range<usize>>,
    pub source_syntax: Arc<Vec<opalglass::syntax::Span>>,
    pub missing_links: Vec<Range<usize>>,
    find_matches: Arc<Vec<Range<usize>>>,
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
            #[cfg(test)]
            render_count: 0,
            #[cfg(test)]
            first_paint_at: None,
            #[cfg(test)]
            first_paint_indexed_rows: None,
            buffer,
            focus: cx.focus_handle(),
            single_line,
            live_preview: false,
            completion_visible: false,
            accepts_attachments: false,
            font_size: 14.,
            line_numbers: true,
            live_revealed: None,
            live_lines: Vec::new(),
            code_blocks: Vec::new(),
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
            source_complete: false,
            source_requested_revision: u64::MAX,
            _source_prepare: None,
            source_list: ListState::new(0, ListAlignment::Top, px(100.)),
            pending_scroll: None,
            scroll_anchor: None,
            source_width: px(0.),
            visible_rows: Vec::new(),
            cached_rows: Vec::new(),
            syntax_version: u64::MAX,
            source_links: Vec::new(),
            source_syntax: Arc::new(Vec::new()),
            missing_links: Vec::new(),
            find_matches: Arc::new(Vec::new()),
        }
    }
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.set_buffer(Buffer::new(opalglass::document::normalize(text)), cx);
    }
    pub fn set_buffer(&mut self, buffer: Buffer, cx: &mut Context<Self>) {
        self.buffer = buffer;
        self.preferred_x = None;
        self.source_version = u64::MAX;
        self.source_complete = false;
        self.source_requested_revision = u64::MAX;
        self._source_prepare = None;
        self.cached_rows.clear();
        self.live_lines.clear();
        self.code_blocks.clear();
        self.source_links.clear();
        self.source_syntax = Arc::new(Vec::new());
        self.missing_links.clear();
        self.source_list.reset(0);
        self.pending_scroll = None;
        self.scroll_anchor = None;
        self.scroll = px(0.);
        self.scroll_x = px(0.);
        self.reveal = true;
        cx.notify();
    }
    pub fn link_query(&self) -> Option<(Range<usize>, String)> {
        if self.single_line
            || !self.buffer.selection().is_empty()
            || self.buffer.marked.is_some()
            || self
                .code_blocks
                .iter()
                .any(|r| r.contains(&self.buffer.cursor()))
        {
            return None;
        }
        let cursor = self.buffer.cursor();
        let start = self.buffer.line_start(cursor);
        let before = &self.buffer.text()[start..cursor];
        let opening = before.rfind("[[")?;
        if opening > 0 && before.as_bytes()[opening - 1] == b'\\' {
            return None;
        }
        let query = &before[opening + 2..];
        if query.contains([']', '|', '#', '[']) || query.len() > 256 {
            return None;
        }
        let after = &self.buffer.text()[cursor..];
        let end = cursor + if after.starts_with("]]") { 2 } else { 0 };
        Some((start + opening..end, query.to_owned()))
    }
    pub fn caret_position(&self) -> Point<Pixels> {
        self.point_for(self.buffer.cursor())
    }
    fn gutter(&self) -> f32 {
        if self.line_numbers { GUTTER } else { PAD }
    }
    pub fn set_preferences(&mut self, font_size: f32, line_numbers: bool, cx: &mut Context<Self>) {
        if self.single_line || (self.font_size == font_size && self.line_numbers == line_numbers) {
            return;
        }
        self.font_size = font_size.clamp(10., 28.);
        self.line_numbers = line_numbers;
        self.cached_rows.clear();
        let offset = self.source_list.logical_scroll_top();
        self.source_list
            .splice(0..self.source_list.item_count(), self.source_ranges.len());
        self.source_list.scroll_to(offset);
        cx.notify();
    }
    pub fn set_find_matches(&mut self, matches: Arc<Vec<Range<usize>>>, cx: &mut Context<Self>) {
        self.find_matches = matches;
        self.cached_rows.clear();
        cx.notify();
    }
    pub fn select_match(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.buffer.select(range.start, range.end);
        self.changed(cx);
    }
    pub fn set_live_preview(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.single_line || self.live_preview == enabled {
            return;
        }
        self.live_preview = enabled;
        self.live_revealed = None;
        self.live_lines.clear();
        self.cached_rows.clear();
        let offset = self.source_list.logical_scroll_top();
        self.source_list
            .splice(0..self.source_list.item_count(), self.source_ranges.len());
        self.source_list.scroll_to(offset);
        cx.notify();
    }
    pub fn set_code_blocks(&mut self, ranges: Vec<Range<usize>>) {
        if self.code_blocks != ranges {
            self.code_blocks = ranges;
            self.live_lines.clear();
            self.cached_rows.clear();
        }
    }
    pub fn saved_scroll(&self) -> opalglass::navigation::ScrollPosition {
        self.pending_scroll.unwrap_or_else(|| {
            let pos = self.source_list.logical_scroll_top();
            opalglass::navigation::ScrollPosition {
                row: pos.item_ix,
                offset: pos.offset_in_item.into(),
            }
        })
    }
    pub fn restore_position(
        &mut self,
        anchor: usize,
        cursor: usize,
        scroll: opalglass::navigation::ScrollPosition,
        cx: &mut Context<Self>,
    ) {
        self.buffer.select(anchor, cursor);
        self.pending_scroll = Some(scroll);
        self.reveal = false;
        cx.notify();
    }
    pub fn scroll_by(&mut self, delta: Pixels, cx: &mut Context<Self>) {
        self.source_list.scroll_by(delta);
        self.reveal = false;
        cx.notify();
    }
    fn publish_scroll_anchor(&mut self, cx: &mut Context<Self>) {
        let offset = self.source_list.logical_scroll_top();
        let Some(range) = self.source_ranges.get(offset.item_ix) else {
            return;
        };
        let height = self
            .cached_rows
            .iter()
            .rev()
            .find(|row| row.range == *range)
            .map_or(px(LINE), |row| row.line.size(row.line_height).height);
        let fraction = (f32::from(offset.offset_in_item) / f32::from(height).max(1.)).clamp(0., 1.);
        let anchor = (
            self.buffer.revision(),
            range.start as f64 + range.len() as f64 * fraction as f64,
        );
        if self.scroll_anchor != Some(anchor) {
            self.scroll_anchor = Some(anchor);
            cx.emit(ScrollAnchor);
        }
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.pending_scroll = None;
        self.reveal = true;
        cx.notify();
    }
    pub fn reveal_cursor(&mut self, cx: &mut Context<Self>) {
        self.changed(cx);
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
        if modifiers.alt && matches!(key.as_str(), "left" | "right") {
            return;
        }
        if self.completion_visible && !command && !modifiers.alt {
            let action = match key.as_str() {
                "up" => Some(CompletionAction::Previous),
                "down" => Some(CompletionAction::Next),
                "enter" | "tab" => Some(CompletionAction::Accept),
                "escape" => Some(CompletionAction::Dismiss),
                _ => None,
            };
            if let Some(action) = action {
                cx.emit(action);
                cx.stop_propagation();
                return;
            }
        }
        if !self.single_line && (modifiers.control || modifiers.platform) && modifiers.alt {
            match key.as_str() {
                "b" => opalglass::editing::wrap(&mut self.buffer, "**"),
                "i" => opalglass::editing::wrap(&mut self.buffer, "*"),
                "c" => opalglass::editing::wrap(&mut self.buffer, "`"),
                _ => return,
            }
            self.changed(cx);
            cx.stop_propagation();
            return;
        }
        if !matches!(key.as_str(), "up" | "down" | "pageup" | "pagedown") {
            self.preferred_x = None;
        }
        if command {
            match key.as_str() {
                "enter" if !self.single_line => opalglass::editing::toggle_task(&mut self.buffer),
                "a" => self.buffer.select(0, self.buffer.text().len()),
                "c" => {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.selected().into()))
                }
                "x" => {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.selected().into()));
                    self.buffer.insert("");
                }
                "v" => {
                    if let Some(item) = cx.read_from_clipboard() {
                        let images: Vec<_> = item
                            .entries()
                            .iter()
                            .filter_map(|entry| match entry {
                                ClipboardEntry::Image(image) => Some(image.clone()),
                                _ => None,
                            })
                            .collect();
                        if self.accepts_attachments && !images.is_empty() {
                            cx.emit(PasteImages(images));
                            cx.stop_propagation();
                            return;
                        }
                        if let Some(text) = item.text() {
                            let text = self.clean_input(&text);
                            self.buffer.insert(&text);
                        }
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
                "enter" if !self.single_line => {
                    let in_code = self
                        .code_blocks
                        .iter()
                        .any(|r| r.contains(&self.buffer.cursor()));
                    if shift || in_code {
                        self.buffer.insert("\n");
                    } else {
                        opalglass::editing::newline(&mut self.buffer);
                    }
                }
                "tab" if !self.single_line => {
                    if shift
                        || !self.buffer.selection().is_empty()
                        || opalglass::editing::list_prefix(
                            &self.buffer.text()[self.buffer.line_start(self.buffer.cursor())
                                ..self.buffer.line_end(self.buffer.cursor())],
                        )
                        .is_some()
                    {
                        opalglass::editing::indent(&mut self.buffer, shift);
                    } else {
                        self.buffer.insert("    ");
                    }
                }
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
                        .position_for_index(
                            row.projection.display_index(byte - row.range.start),
                            row.line_height,
                        )
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
                .find(|r| position.y < r.origin.y + r.line.size(r.line_height).height)
                .or_else(|| self.visible_rows.last())
            {
                let local = point(
                    position.x - row.origin.x,
                    (position.y - row.origin.y).max(px(0.)),
                );
                let display = row
                    .line
                    .closest_index_for_position(local, row.line_height)
                    .unwrap_or_else(|i| i);
                return row.range.start + row.projection.source_index(display).min(row.range.len());
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
        let height = if !self.single_line {
            self.prepare_source();
            let index = self
                .source_ranges
                .partition_point(|r| r.start <= self.buffer.cursor())
                .saturating_sub(1);
            self.shaped_source_row(index, window, cx).line_height
        } else {
            px(LINE)
        };
        self.move_vertical(if down { height } else { -height }, extend, window, cx);
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
            .line
            .position_for_index(
                line.projection
                    .display_index(cursor - self.source_ranges[index].start),
                line.line_height,
            )
            .unwrap_or_default();
        let x = *self.preferred_x.get_or_insert(pos.x);
        let mut y = pos.y + line.line_height / 2. + distance;
        loop {
            let height = line
                .line
                .size(line.line_height)
                .height
                .max(line.line_height);
            if y < px(0.) && index > 0 {
                index -= 1;
                line = self.shaped_source_row(index, window, cx);
                y += line
                    .line
                    .size(line.line_height)
                    .height
                    .max(line.line_height);
            } else if y >= height && index + 1 < self.source_ranges.len() {
                y -= height;
                index += 1;
                line = self.shaped_source_row(index, window, cx);
            } else {
                let range = &self.source_ranges[index];
                let offset = if y < px(0.) {
                    0
                } else if y >= height {
                    line.projection.text.len()
                } else {
                    line.line
                        .closest_index_for_position(point(x, y), line.line_height)
                        .unwrap_or_else(|i| i)
                };
                self.buffer.move_to(
                    range.start + line.projection.source_index(offset).min(range.len()),
                    extend,
                );
                break;
            }
        }
    }
    fn shaped_source_row(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> CachedRow {
        let _ = self.source_row(index, window, cx);
        self.cached_rows
            .iter()
            .rev()
            .find(|r| r.range == self.source_ranges[index])
            .unwrap()
            .clone()
    }
    pub fn place_drop_caret(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.bounds.contains(&position) {
            self.buffer.move_to(self.index_for(position), false);
            self.reveal_cursor(cx);
        }
    }
    pub fn set_source_links(&mut self, links: Vec<Range<usize>>, missing: Vec<Range<usize>>) {
        self.syntax_version = self.buffer.revision();
        self.source_links = links;
        self.missing_links = missing;
        self.cached_rows.clear();
    }
    fn prepare_source(&mut self) {
        if self.single_line {
            return;
        }
        if self.source_version != self.buffer.revision() {
            if self.syntax_version != self.buffer.revision() {
                self.source_links.clear();
                self.source_syntax = Arc::new(Vec::new());
                self.missing_links.clear();
            }
            let offset = self.source_list.logical_scroll_top();
            self._source_prepare = None;
            if self.source_version == u64::MAX {
                (self.source_ranges, self.source_complete) =
                    opalglass::document::opening_source_rows(self.buffer.text());
            } else {
                self.source_ranges = opalglass::document::source_rows(self.buffer.text());
                self.source_complete = true;
            }
            self.source_list
                .splice(0..self.source_list.item_count(), self.source_ranges.len());
            self.source_list.scroll_to(offset);
            self.source_version = self.buffer.revision();
            self.find_matches = Arc::new(Vec::new());
            self.cached_rows.clear();
            self.live_lines.clear();
        }
        if self.live_preview {
            let selection = self.buffer.selection();
            let revealed =
                self.buffer.line_start(selection.start)..self.buffer.line_end(selection.end) + 1;
            if self.live_revealed.as_ref() != Some(&revealed) {
                let offset = self.source_list.logical_scroll_top();
                for range in self.live_revealed.iter().chain(std::iter::once(&revealed)) {
                    let start = self.source_ranges.partition_point(|r| r.end < range.start);
                    let end = self.source_ranges.partition_point(|r| r.start < range.end);
                    if end > start {
                        self.source_list.splice(start..end, end - start);
                    }
                }
                self.source_list.scroll_to(offset);
                self.live_revealed = Some(revealed);
            }
        }
        // Explicit navigation outside the opening viewport must work even if
        // the background index has not arrived yet (Ctrl+End, heading jumps).
        if !self.source_complete
            && self.pending_scroll.is_none()
            && self.buffer.cursor() >= self.source_ranges.last().map_or(0, |r| r.end)
        {
            let ranges = opalglass::document::source_rows(self.buffer.text());
            self.finish_source_index(ranges);
            self._source_prepare = None;
        }
        if let Some(pos) = self.pending_scroll
            && (self.source_complete || pos.row < self.source_ranges.len())
        {
            self.source_list.scroll_to(ListOffset {
                item_ix: pos.row.min(self.source_ranges.len().saturating_sub(1)),
                offset_in_item: px(pos.offset),
            });
            self.pending_scroll = None;
        }
        if self.reveal {
            let index = self
                .source_ranges
                .partition_point(|r| r.start <= self.buffer.cursor())
                .saturating_sub(1);
            let visible = self.visible_rows.iter().any(|r| {
                r.range.start == self.source_ranges[index].start
                    && r.origin.y >= self.bounds.top()
                    && r.origin.y + r.line_height <= self.bounds.bottom()
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
    fn finish_source_index(&mut self, ranges: Vec<Range<usize>>) {
        let count = self.source_ranges.len();
        debug_assert_eq!(&ranges[..count], self.source_ranges.as_slice());
        // Keep measured viewport rows and its scroll offset intact; append only
        // the newly indexed tail rather than rebuilding the whole list.
        self.source_list.splice(count..count, ranges.len() - count);
        self.source_ranges = ranges;
        self.source_complete = true;
    }
    fn start_source_index(&mut self, revision: u64, cx: &mut Context<Self>) {
        if self.source_complete || self.buffer.revision() != revision {
            return;
        }
        let text = self.buffer.snapshot();
        self._source_prepare = Some(cx.spawn(async move |this, cx| {
            let ranges = cx
                .background_executor()
                .spawn(async move { opalglass::document::source_rows(&text) })
                .await;
            let _ = this.update(cx, |editor, cx| {
                if editor.buffer.revision() != revision || editor.source_complete {
                    return;
                }
                editor.finish_source_index(ranges);
                cx.notify();
            });
        }));
    }
    fn projection(&mut self, range: &Range<usize>, revealed: bool) -> Arc<live::Line> {
        if !self.live_preview {
            return Arc::new(live::Line::raw(&self.buffer.text()[range.clone()]));
        }
        let start = self.buffer.line_start(range.start);
        let end = self.buffer.line_end(range.start);
        // Bound even a pathological single-line note. Normal lines are parsed as
        // a unit so inline markup can cross the 256-byte virtual row boundaries.
        let physical = if end - start <= 8192 {
            start..end
        } else {
            range.clone()
        };
        let cached = self
            .live_lines
            .iter()
            .find(|(r, active, _)| *r == physical && *active == revealed)
            .map(|(_, _, line)| line.clone());
        let line = cached.unwrap_or_else(|| {
            let i = self.code_blocks.partition_point(|r| r.end <= range.start);
            let code = self
                .code_blocks
                .get(i)
                .is_some_and(|r| r.contains(&range.start));
            let line = Arc::new(live::Line::new(
                &self.buffer.text()[physical.clone()],
                revealed,
                code,
                physical.start != start,
            ));
            if self.live_lines.len() >= 128 {
                self.live_lines.drain(..64);
            }
            self.live_lines
                .push((physical.clone(), revealed, line.clone()));
            line
        });
        if physical == *range {
            line
        } else {
            Arc::new(line.slice(range.start - physical.start..range.end - physical.start))
        }
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[path = "editor/input.rs"]
mod input;
#[path = "editor/rendering.rs"]
mod rendering;
