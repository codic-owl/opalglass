//! Original editing model. All internal positions are UTF-8 byte boundaries;
//! platform text services use the explicit UTF-16 conversion methods.
use std::{cell::Cell, collections::VecDeque, ops::Range, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;

fn next_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: Arc<String>,
    anchor: usize,
    cursor: usize,
}

#[derive(Clone, Debug)]
struct Edit {
    start: usize,
    removed: String,
    inserted: String,
    before: (usize, usize),
    after: (usize, usize),
}
impl Edit {
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

#[derive(Clone, Copy, Debug)]
struct CachedPosition {
    revision: u64,
    cursor: usize,
    position: (usize, usize),
}

#[derive(Clone, Debug)]
pub struct Buffer {
    state: Snapshot,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    history_bytes: usize,
    revision: u64,
    line_starts: Vec<usize>,
    line_utf16: Vec<usize>,
    pub marked: Option<Range<usize>>,
    composition_start: Option<Snapshot>,
    preferred_column: Option<usize>,
    position_cache: Cell<Option<CachedPosition>>,
}

impl Buffer {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        let mut line_utf16 = vec![0];
        let mut byte = 0;
        let mut utf16 = 0;
        for line in text.split_inclusive('\n') {
            byte += line.len();
            utf16 += if line.is_ascii() {
                line.len()
            } else {
                line.encode_utf16().count()
            };
            if line.ends_with('\n') {
                line_starts.push(byte);
                line_utf16.push(utf16);
            }
        }
        Self {
            state: Snapshot {
                text: Arc::new(text),
                anchor: 0,
                cursor: 0,
            },
            undo: VecDeque::new(),
            redo: Vec::new(),
            history_bytes: 0,
            revision: next_revision(),
            line_starts,
            line_utf16,
            marked: None,
            composition_start: None,
            preferred_column: None,
            position_cache: Cell::new(None),
        }
    }
    pub fn text(&self) -> &str {
        &self.state.text
    }
    pub fn snapshot(&self) -> Arc<String> {
        self.state.text.clone()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn cursor(&self) -> usize {
        self.state.cursor
    }
    pub fn anchor(&self) -> usize {
        self.state.anchor
    }
    pub fn selection(&self) -> Range<usize> {
        self.anchor().min(self.cursor())..self.anchor().max(self.cursor())
    }
    pub fn selected(&self) -> &str {
        &self.text()[self.selection()]
    }
    fn boundary(&self, index: usize) -> usize {
        let mut index = index.min(self.text().len());
        while !self.text().is_char_boundary(index) {
            index -= 1;
        }
        index
    }
    pub fn select(&mut self, anchor: usize, cursor: usize) {
        self.state.anchor = self.boundary(anchor);
        self.state.cursor = self.boundary(cursor);
        self.preferred_column = None;
    }
    pub fn move_to(&mut self, index: usize, extend: bool) {
        self.select(if extend { self.anchor() } else { index }, index);
    }
    fn remember(&mut self, edit: Edit) {
        self.history_bytes += edit.bytes();
        self.undo.push_back(edit);
        self.redo.clear();
        // Keep the latest transaction even if it alone exceeds the budget.
        while self.undo.len() > 1
            && (self.undo.len() > 200 || self.history_bytes > 32 * 1024 * 1024)
        {
            self.history_bytes -= self.undo.pop_front().unwrap().bytes();
        }
    }
    fn replace_raw(&mut self, range: Range<usize>, text: &str) {
        let start = self.boundary(range.start);
        let end = self.boundary(range.end).max(start);
        if &self.text()[start..end] != text {
            let first = self.line_starts.partition_point(|&b| b <= start) - 1;
            let last = self.line_starts.partition_point(|&b| b <= end) - 1;
            let utf_start = self.byte_to_utf16(start);
            let removed_utf = self.text()[start..end].encode_utf16().count();
            let inserted_utf = text.encode_utf16().count();
            for b in &mut self.line_starts[last + 1..] {
                *b = *b - (end - start) + text.len();
            }
            for u in &mut self.line_utf16[last + 1..] {
                *u = *u - removed_utf + inserted_utf;
            }
            let mut starts = Vec::new();
            let mut units = Vec::new();
            let mut b = start;
            let mut u = utf_start;
            for line in text.split_inclusive('\n') {
                b += line.len();
                u += line.encode_utf16().count();
                if line.ends_with('\n') {
                    starts.push(b);
                    units.push(u);
                }
            }
            self.line_starts.splice(first + 1..last + 1, starts);
            self.line_utf16.splice(first + 1..last + 1, units);
            Arc::make_mut(&mut self.state.text).replace_range(start..end, text);
            self.revision = next_revision();
        }
        self.move_to(start + text.len(), false);
    }
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        let start = self.boundary(range.start);
        let end = self.boundary(range.end).max(start);
        if let Some(before) = self.composition_start.take() {
            self.marked = None;
            self.replace_raw(start..end, text);
            self.remember_composition(before);
            return;
        }
        self.marked = None;
        let removed = self.text()[start..end].to_owned();
        let before = (self.anchor(), self.cursor());
        self.replace_raw(start..end, text);
        if removed != text {
            self.remember(Edit {
                start,
                removed,
                inserted: text.into(),
                before,
                after: (self.anchor(), self.cursor()),
            });
        }
    }
    pub fn insert(&mut self, text: &str) {
        self.replace(self.selection(), text);
    }
    pub fn compose(&mut self, range: Option<Range<usize>>, text: &str) {
        if self.composition_start.is_none() {
            self.composition_start = Some(self.state.clone());
        }
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection());
        let start = self.boundary(range.start);
        self.replace_raw(range, text);
        self.marked = Some(start..start + text.len());
    }
    fn remember_composition(&mut self, before: Snapshot) {
        if before.text == self.state.text {
            return;
        }
        let mut start = before
            .text
            .bytes()
            .zip(self.text().bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !before.text.is_char_boundary(start) || !self.text().is_char_boundary(start) {
            start -= 1;
        }
        let mut suffix = before.text[start..]
            .bytes()
            .rev()
            .zip(self.text()[start..].bytes().rev())
            .take_while(|(a, b)| a == b)
            .count();
        while !before.text.is_char_boundary(before.text.len() - suffix)
            || !self.text().is_char_boundary(self.text().len() - suffix)
        {
            suffix -= 1;
        }
        self.remember(Edit {
            start,
            removed: before.text[start..before.text.len() - suffix].into(),
            inserted: self.text()[start..self.text().len() - suffix].into(),
            before: (before.anchor, before.cursor),
            after: (self.anchor(), self.cursor()),
        });
    }
    pub fn finish_composition(&mut self) {
        self.marked = None;
        if let Some(before) = self.composition_start.take() {
            self.remember_composition(before);
        }
    }
    pub fn undo(&mut self) {
        self.finish_composition();
        if let Some(edit) = self.undo.pop_back() {
            self.history_bytes -= edit.bytes();
            self.replace_raw(edit.start..edit.start + edit.inserted.len(), &edit.removed);
            self.select(edit.before.0, edit.before.1);
            self.redo.push(edit);
        }
        self.preferred_column = None;
    }
    pub fn redo(&mut self) {
        if let Some(edit) = self.redo.pop() {
            self.replace_raw(edit.start..edit.start + edit.removed.len(), &edit.inserted);
            self.select(edit.after.0, edit.after.1);
            self.history_bytes += edit.bytes();
            self.undo.push_back(edit);
        }
        self.preferred_column = None;
    }
    pub fn previous(&self, index: usize) -> usize {
        self.text()[..self.boundary(index)]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }
    pub fn next(&self, index: usize) -> usize {
        let index = self.boundary(index);
        self.text()[index..]
            .graphemes(true)
            .next()
            .map_or(index, |s| index + s.len())
    }
    pub fn horizontal(&mut self, right: bool, extend: bool) {
        let range = self.selection();
        let index = if !extend && !range.is_empty() {
            if right { range.end } else { range.start }
        } else if right {
            self.next(self.cursor())
        } else {
            self.previous(self.cursor())
        };
        self.move_to(index, extend);
    }
    /// Move over a Unicode word or punctuation segment, skipping adjacent space.
    pub fn word_boundary(&self, right: bool) -> usize {
        let cursor = self.cursor();
        if right {
            self.text()[cursor..]
                .split_word_bound_indices()
                .find(|(_, part)| !part.chars().all(char::is_whitespace))
                .map_or(self.text().len(), |(i, part)| cursor + i + part.len())
        } else {
            self.text()[..cursor]
                .split_word_bound_indices()
                .rev()
                .find(|(_, part)| !part.chars().all(char::is_whitespace))
                .map_or(0, |(i, _)| i)
        }
    }
    pub fn move_word(&mut self, right: bool, extend: bool) {
        self.move_to(self.word_boundary(right), extend);
    }
    pub fn delete_word(&mut self, backwards: bool) {
        if self.selection().is_empty() {
            self.move_word(!backwards, true);
        }
        self.insert("");
    }
    pub fn line_number(&self, index: usize) -> usize {
        self.line_starts
            .partition_point(|&b| b <= self.boundary(index))
    }
    pub fn delete(&mut self, backwards: bool) {
        let mut range = self.selection();
        if range.is_empty() {
            if backwards {
                range.start = self.previous(range.start);
            } else {
                range.end = self.next(range.end);
            }
        }
        self.replace(range, "");
    }
    pub fn line_start(&self, index: usize) -> usize {
        self.line_starts[self
            .line_starts
            .partition_point(|&b| b <= self.boundary(index))
            - 1]
    }
    pub fn line_end(&self, index: usize) -> usize {
        self.line_starts
            .get(
                self.line_starts
                    .partition_point(|&b| b <= self.boundary(index)),
            )
            .map_or(self.text().len(), |b| b - 1)
    }
    pub fn vertical(&mut self, down: bool, extend: bool) {
        let start = self.line_start(self.cursor());
        let column = self
            .preferred_column
            .unwrap_or_else(|| self.text()[start..self.cursor()].graphemes(true).count());
        let target_start = if down {
            let end = self.line_end(self.cursor());
            if end == self.text().len() {
                self.move_to(end, extend);
                return;
            }
            end + 1
        } else {
            if start == 0 {
                self.move_to(0, extend);
                return;
            }
            self.line_start(start - 1)
        };
        let end = self.line_end(target_start);
        let target = self.text()[target_start..end]
            .grapheme_indices(true)
            .nth(column)
            .map_or(end, |(i, _)| target_start + i);
        self.move_to(target, extend);
        self.preferred_column = Some(column);
    }
    pub fn byte_to_utf16(&self, byte: usize) -> usize {
        let byte = self.boundary(byte);
        let line = self.line_starts.partition_point(|&b| b <= byte) - 1;
        self.line_utf16[line]
            + self.text()[self.line_starts[line]..byte]
                .encode_utf16()
                .count()
    }
    pub fn utf16_to_byte(&self, offset: usize) -> usize {
        let line = self.line_utf16.partition_point(|&u| u <= offset) - 1;
        let mut count = self.line_utf16[line];
        for (byte, c) in self.text()[self.line_starts[line]..].char_indices() {
            if count + c.len_utf16() > offset {
                return self.line_starts[line] + byte;
            }
            count += c.len_utf16();
        }
        self.text().len()
    }
    pub fn from_utf16(&self, range: Range<usize>) -> Range<usize> {
        self.utf16_to_byte(range.start)..self.utf16_to_byte(range.end)
    }
    pub fn to_utf16(&self, range: Range<usize>) -> Range<usize> {
        self.byte_to_utf16(range.start)..self.byte_to_utf16(range.end)
    }
    pub fn line_column(&self) -> (usize, usize) {
        if let Some(cached) = self.position_cache.get()
            && cached.revision == self.revision
            && cached.cursor == self.cursor()
        {
            return cached.position;
        }
        let position = (
            self.line_starts.partition_point(|&b| b <= self.cursor()),
            self.text()[self.line_start(self.cursor())..self.cursor()]
                .graphemes(true)
                .count()
                + 1,
        );
        self.position_cache.set(Some(CachedPosition {
            revision: self.revision,
            cursor: self.cursor(),
            position,
        }));
        position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_position_tracks_unicode_cursor_edits_and_history() {
        let mut buffer = Buffer::new("e\u{301}😀\nend");
        buffer.move_to("e\u{301}😀".len(), false);
        assert_eq!(buffer.line_column(), (1, 3));
        assert_eq!(buffer.line_column(), (1, 3));
        buffer.move_to(buffer.text().len(), false);
        assert_eq!(buffer.line_column(), (2, 4));
        buffer.replace(0..buffer.text().len(), "a\nb");
        assert_eq!(buffer.line_column(), (2, 2));
        buffer.undo();
        assert_eq!(buffer.line_column(), (2, 4));
        buffer.redo();
        assert_eq!(buffer.line_column(), (2, 2));
        buffer.move_to(0, false);
        assert_eq!(buffer.line_column(), (1, 1));
    }
    #[test]
    fn word_navigation_and_deletion_preserve_unicode_and_undo() {
        let text = "café e\u{301}lan,  世界\nnext";
        let mut buffer = Buffer::new(text);
        buffer.move_word(true, false);
        assert_eq!(&buffer.text()[..buffer.cursor()], "café");
        buffer.move_word(true, true);
        assert_eq!(buffer.selected(), " e\u{301}lan");
        buffer.delete_word(true);
        assert_eq!(buffer.text(), "café,  世界\nnext");
        buffer.undo();
        assert_eq!(buffer.text(), text);
        assert_eq!(buffer.selected(), " e\u{301}lan");
        buffer.move_to(text.len(), false);
        buffer.delete_word(true);
        assert_eq!(buffer.text(), "café e\u{301}lan,  世界\n");
        buffer.undo();
        buffer.move_to(0, false);
        buffer.delete_word(false);
        assert_eq!(buffer.text(), " e\u{301}lan,  世界\nnext");
        buffer.move_word(false, false);
        assert_eq!(buffer.cursor(), 0);
        buffer.move_to(buffer.text().len(), false);
        buffer.move_word(true, false);
        assert_eq!(buffer.cursor(), buffer.text().len());
    }
    #[test]
    fn indexed_positions_match_unicode_reference_after_replacements_and_history() {
        let mut buffer = Buffer::new("first😀\nsecond\nş\n");
        let mut seed = 17_u64;
        let mut previous = Vec::new();
        for _ in 0..150 {
            let boundaries: Vec<_> = buffer
                .text()
                .char_indices()
                .map(|(i, _)| i)
                .chain([buffer.text().len()])
                .collect();
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let a = boundaries[seed as usize % boundaries.len()];
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let b = boundaries[seed as usize % boundaries.len()];
            let text = ["😀\n", "ş", "\n\n", "x", ""][seed as usize % 5];
            let range = a.min(b)..a.max(b);
            let mut expected = buffer.text().to_owned();
            expected.replace_range(range.clone(), text);
            if expected != buffer.text() {
                previous.push(buffer.text().to_owned());
            }
            buffer.replace(range, text);
            assert_eq!(buffer.text(), expected);
            let fresh = Buffer::new(expected);
            assert_eq!(buffer.line_starts, fresh.line_starts);
            assert_eq!(buffer.line_utf16, fresh.line_utf16);
            for byte in buffer
                .text()
                .char_indices()
                .map(|(i, _)| i)
                .chain([buffer.text().len()])
            {
                let units = buffer.text()[..byte].encode_utf16().count();
                assert_eq!(buffer.byte_to_utf16(byte), units);
                assert_eq!(buffer.utf16_to_byte(units), byte);
            }
        }
        let end = buffer.text().to_owned();
        for text in previous.iter().rev() {
            buffer.undo();
            assert_eq!(buffer.text(), text);
        }
        for _ in previous {
            buffer.redo();
        }
        assert_eq!(buffer.text(), end);
    }

    #[test]
    fn small_edits_have_small_history_and_snapshots_stay_immutable() {
        let mut buffer = Buffer::new("x".repeat(1_000_000));
        let snapshot = buffer.snapshot();
        buffer.move_to(buffer.text().len(), false);
        for _ in 0..200 {
            buffer.insert("a");
        }
        assert_eq!(buffer.history_bytes, 200);
        assert_eq!(snapshot.len(), 1_000_000);
        for _ in 0..200 {
            buffer.undo();
        }
        assert_eq!(buffer.text(), snapshot.as_str());
    }
    #[test]
    fn graphemes_delete_as_one_and_undo_restores_selection() {
        let mut b = Buffer::new("a👩‍💻e\u{301}");
        b.move_to(b.text().len(), false);
        b.delete(true);
        assert_eq!(b.text(), "a👩‍💻");
        b.delete(true);
        assert_eq!(b.text(), "a");
        b.undo();
        b.undo();
        assert_eq!(b.text(), "a👩‍💻e\u{301}");
        assert_eq!(b.cursor(), b.text().len());
        b.redo();
        assert_eq!(b.text(), "a👩‍💻");
    }
    #[test]
    fn utf16_handles_surrogates_and_non_ascii() {
        let b = Buffer::new("a😀ş");
        assert_eq!(b.from_utf16(1..3), 1..5);
        assert_eq!(b.to_utf16(5..7), 3..4);
        assert_eq!(b.utf16_to_byte(2), 1);
    }
    #[test]
    fn composition_is_one_undo_transaction() {
        let mut b = Buffer::new("");
        b.compose(None, "n");
        b.compose(None, "ni");
        b.replace(b.marked.clone().unwrap(), "你");
        assert_eq!(b.text(), "你");
        b.undo();
        assert_eq!(b.text(), "");
        b.redo();
        assert_eq!(b.text(), "你");
    }
    #[test]
    fn vertical_navigation_remembers_column() {
        let mut b = Buffer::new("abcde\nx\nabcde");
        b.move_to(4, false);
        b.vertical(true, false);
        assert_eq!(b.cursor(), 7);
        b.vertical(true, true);
        assert_eq!(b.cursor(), 12);
        assert_eq!(b.selected(), "\nabcd");
    }
    #[test]
    fn replacing_selection_and_branching_discards_redo() {
        let mut b = Buffer::new("one two");
        b.select(4, 7);
        b.insert("three");
        b.undo();
        assert_eq!(b.selected(), "two");
        b.insert("four");
        b.redo();
        assert_eq!(b.text(), "one four");
    }
}
