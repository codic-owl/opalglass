//! Compact exact Source checkpoints prepared outside the UI/window caches.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const STRIDE: usize = 64;
const WINDOW_ROWS: usize = 32;

pub(super) fn prepare(
    text: &str,
    system: &Arc<TextSystem>,
    width: Pixels,
    font_size: f32,
    cancelled: &AtomicBool,
) -> Option<Vec<(usize, usize)>> {
    let rows = opalglass::document::source_rows(text);
    let mut result = Vec::new();
    let mut index = 0;
    let mut session = system.layout_session();
    let mut windows = 0;
    while index < rows.len() {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let first = index;
        let end = text[rows[index].start..]
            .find('\n')
            .map_or(text.len(), |offset| rows[index].start + offset);
        if rows[index].end == end {
            index += 1;
            continue;
        }
        let last = rows.partition_point(|row| row.start < end);
        let mut byte = rows[index].start;
        let mut wraps = Vec::new();
        let mut stop = byte;
        while index < last {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            if index == first || (index - first).is_multiple_of(STRIDE) {
                result.push((index, byte));
            }
            let target = rows[index].end;
            let next = if byte >= target {
                byte
            } else if target == end {
                end
            } else {
                if wraps.last().is_none_or(|last| *last <= byte) {
                    // Retain at most eight bounded windows, then release every
                    // native layout. Repeated note text can reuse exact shapes.
                    if windows == 8 {
                        session = system.layout_session();
                        windows = 0;
                    }
                    stop = rows[(index + WINDOW_ROWS).min(last - 1)].end;
                    wraps = shape_window(text, byte..stop, &session, width, font_size)?;
                    windows += 1;
                }
                let available = &wraps[wraps.partition_point(|wrap| *wrap <= byte)..];
                available
                    .iter()
                    .copied()
                    .take_while(|wrap| *wrap <= target)
                    .last()
                    .or_else(|| available.first().copied())
                    .or((stop == end).then_some(end))?
            };
            byte = next;
            index += 1;
        }
    }
    Some(result)
}

fn shape_window(
    text: &str,
    range: Range<usize>,
    session: &WindowTextSystem,
    width: Pixels,
    font_size: f32,
) -> Option<Vec<usize>> {
    let source = &text[range.clone()];
    let line = session
        .shape_text(
            source.to_owned().into(),
            px(font_size),
            &[TextRun {
                len: source.len(),
                font: font(MONO_FONT),
                color: rgb(0xd6d9e3).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            Some(width),
            None,
        )
        .ok()?
        .into_iter()
        .next()?;
    let graphemes = (!source.is_ascii()).then(|| opalglass::document::chunks(source, 1));
    Some(
        line.wrap_boundaries
            .iter()
            .map(|boundary| {
                line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
            })
            .filter(|byte| {
                graphemes.as_ref().is_none_or(|graphemes| {
                    graphemes
                        .binary_search_by_key(byte, |grapheme| grapheme.start)
                        .is_ok()
                })
            })
            .map(|byte| range.start + byte)
            .collect(),
    )
}
