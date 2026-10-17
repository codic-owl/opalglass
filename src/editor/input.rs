// GPUI input-method adapter; byte/UTF-16 conversion stays in the buffer.
use super::*;

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
        let height = self
            .visible_rows
            .iter()
            .find(|r| r.range.start <= byte && byte <= r.range.end)
            .map_or(px(LINE), |r| r.line_height);
        Some(Bounds::new(self.point_for(byte), size(px(2.), height)))
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
