//! Background-prepared slices of a table row. The UI only shapes visible slices;
//! the complete cells and their styles are retained once for all slices.
use crate::{document, markdown::Span, rendering::TableRow};
use std::{ops::Range, sync::Arc};

const CELL_BYTES: usize = 1024;
// Malformed or generated text can contain an arbitrarily long grapheme. Reading
// may split that exceptional cluster, but never passes it unbounded to shaping.
pub const MAX_CELL_BYTES: usize = 4096;

#[derive(Clone, Debug)]
pub struct Chunk {
    pub row: Arc<TableRow>,
    ranges: Arc<Vec<Vec<Range<usize>>>>,
    index: usize,
    pub first: bool,
    pub last: bool,
}

impl Chunk {
    pub fn text(&self, column: usize) -> &str {
        &self.row.cells[column].text[self.range(column)]
    }

    pub fn spans(&self, column: usize) -> Vec<Span> {
        let range = self.range(column);
        if range.is_empty() {
            return Vec::new();
        }
        let spans = &self.row.cells[column].spans;
        let first = spans.partition_point(|s| s.range.end <= range.start);
        spans[first..]
            .iter()
            .take_while(|s| s.range.start < range.end)
            .map(|span| Span {
                range: span.range.start.max(range.start) - range.start
                    ..span.range.end.min(range.end) - range.start,
                style: span.style.clone(),
            })
            .collect()
    }

    fn range(&self, column: usize) -> Range<usize> {
        let end = self.row.cells[column].text.len();
        self.ranges[column]
            .get(self.index)
            .cloned()
            .unwrap_or(end..end)
    }
}

pub fn chunks(row: TableRow) -> Vec<Arc<Chunk>> {
    let ranges: Arc<Vec<_>> = Arc::new(
        row.cells
            .iter()
            .map(|cell| cell_ranges(&cell.text))
            .collect(),
    );
    let count = ranges.iter().map(Vec::len).max().unwrap_or(1);
    let row = Arc::new(row);
    (0..count)
        .map(|index| {
            Arc::new(Chunk {
                ranges: ranges.clone(),
                index,
                row: row.clone(),
                first: index == 0,
                last: index + 1 == count,
            })
        })
        .collect()
}

fn cell_ranges(text: &str) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    for range in document::chunks(text, CELL_BYTES) {
        let mut start = range.start;
        while range.end - start > MAX_CELL_BYTES {
            let mut end = start + MAX_CELL_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            result.push(start..end);
            start = end;
        }
        result.push(start..range.end);
    }
    result
}

#[cfg(test)]
mod tests;
