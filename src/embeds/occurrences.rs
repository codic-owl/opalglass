//! Index syntactic occurrences during background preparation, before UI frames.
use super::{Request, requests};
use crate::prepared::PreviewRow;
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct Occurrence {
    /// Original prepared row interval. One long formatted label may span chunks.
    pub rows: Range<usize>,
    pub request: Request,
    pub last_end: usize,
}

pub fn occurrences(rows: &[PreviewRow]) -> Vec<Occurrence> {
    let mut result: Vec<Occurrence> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        for request in requests(row) {
            if let Some(previous) = result.last_mut()
                && previous.rows.end == index
                && row.continuation
                && request.range.start == 0
                && previous.last_end == rows[index - 1].text.len()
                && previous.request.target == request.target
                && previous.request.image_start == request.image_start
            {
                previous.rows.end = index + 1;
                previous.last_end = request.range.end;
            } else {
                result.push(Occurrence {
                    rows: index..index + 1,
                    last_end: request.range.end,
                    request,
                });
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;
