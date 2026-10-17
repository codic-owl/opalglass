//! Source geometry, vertical movement and persistent physical scroll positions.
use super::*;

impl Editor {
    pub fn saved_scroll(&self) -> opalglass::navigation::ScrollPosition {
        self.pending_scroll.unwrap_or_else(|| {
            let pos = self.source_list.logical_scroll_top();
            opalglass::navigation::ScrollPosition {
                row: self.visibility.rows.row(pos.item_ix),
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
    pub(super) fn publish_scroll_anchor(&mut self, cx: &mut Context<Self>) {
        let offset = self.source_list.logical_scroll_top();
        let physical = self.visibility.rows.row(offset.item_ix);
        let Some(range) = self.source_ranges.get(physical) else {
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
    pub(super) fn origin(&self) -> Point<Pixels> {
        self.bounds.origin
            + point(
                px(if self.single_line { 12. } else { PAD }) - self.scroll_x,
                px(if self.single_line { 7. } else { PAD }) - self.scroll,
            )
    }
    pub(super) fn point_for(&self, byte: usize) -> Point<Pixels> {
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
    pub(super) fn index_for(&self, position: Point<Pixels>) -> usize {
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
    pub(super) fn visual_move(
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
    pub(super) fn move_vertical(
        &mut self,
        distance: Pixels,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_source();
        let cursor = self.buffer.cursor();
        let mut physical = self
            .source_ranges
            .partition_point(|r| r.start <= cursor)
            .saturating_sub(1);
        let mut index = self.visibility.rows.list_index(physical);
        physical = self.visibility.rows.row(index);
        let mut line = self.shaped_source_row(physical, window, cx);
        let pos = line
            .line
            .position_for_index(
                line.projection
                    .display_index(cursor.saturating_sub(self.source_ranges[physical].start)),
                line.line_height,
            )
            .unwrap_or_default();
        let indent = self.callout_indent(self.source_ranges[physical].start);
        let x = *self.preferred_x.get_or_insert(pos.x + indent);
        let mut y = pos.y + line.line_height / 2. + distance;
        loop {
            let height = line
                .line
                .size(line.line_height)
                .height
                .max(line.line_height);
            if y < px(0.) && index > 0 {
                index -= 1;
                physical = self.visibility.rows.row(index);
                line = self.shaped_source_row(physical, window, cx);
                y += line
                    .line
                    .size(line.line_height)
                    .height
                    .max(line.line_height);
            } else if y >= height && index + 1 < self.visibility.rows.len() {
                y -= height;
                index += 1;
                physical = self.visibility.rows.row(index);
                line = self.shaped_source_row(physical, window, cx);
            } else {
                let range = &self.source_ranges[physical];
                let offset = if y < px(0.) {
                    0
                } else if y >= height {
                    line.projection.text.len()
                } else {
                    let local_x = (x - self.callout_indent(range.start)).max(px(0.));
                    line.line
                        .closest_index_for_position(point(local_x, y), line.line_height)
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
    pub(super) fn shaped_source_row(
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
}
