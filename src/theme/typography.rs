//! Raised footnote glyphs over the original text layout retain native hit testing.
use gpui::{prelude::*, *};
use opalglass::markdown::{InlineStyle, Span};
use std::ops::Range;

pub(crate) fn is_reference(style: &InlineStyle) -> bool {
    style
        .target
        .as_deref()
        .is_some_and(|target| target.starts_with(opalglass::footnotes::TARGET_PREFIX))
}

/// The normal text keeps its width and byte indices, while these smaller glyphs
/// supply the visible reference. Selection, wrapping and mouse targets therefore
/// continue to use the same source mapping as the surrounding paragraph.
pub(crate) fn paint_references(
    text: &str,
    spans: &[Span],
    style: &TextStyle,
    line_height: Pixels,
    position: impl Fn(usize) -> Option<Point<Pixels>>,
    window: &mut Window,
    cx: &mut App,
) {
    let font_size = style.font_size.to_pixels(window.rem_size());
    for span in spans.iter().filter(|span| is_reference(&span.style)) {
        let Some(value) = text.get(span.range.clone()) else {
            continue;
        };
        let mut font = style.font();
        if span.style.bold {
            font.weight = FontWeight::BOLD;
        }
        if span.style.italic {
            font.style = FontStyle::Italic;
        }
        // A reference may wrap in a very narrow pane or be split by a prepared
        // row. Paint each actual wrapped part without crossing its line edge.
        for (range, origin) in segments(value, span.range.start, &position) {
            let text: SharedString = value[range].to_owned().into();
            let line = window.text_system().shape_line(
                text.clone(),
                font_size * 0.75,
                &[TextRun {
                    len: text.len(),
                    font: font.clone(),
                    color: rgb(0xb8a5fa).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            );
            let origin = origin - point(px(0.), font_size * 0.28);
            let _ = line.paint(origin, line_height, window, cx);
        }
    }
}

fn segments(
    text: &str,
    offset: usize,
    position: &impl Fn(usize) -> Option<Point<Pixels>>,
) -> Vec<(Range<usize>, Point<Pixels>)> {
    let mut result: Vec<(Range<usize>, Point<Pixels>)> = Vec::new();
    for (index, ch) in text.char_indices() {
        let Some(origin) = position(offset + index) else {
            continue;
        };
        if let Some((range, previous)) = result.last_mut()
            && previous.y == origin.y
            && range.end == index
        {
            range.end = index + ch.len_utf8();
        } else {
            result.push((index..index + ch.len_utf8(), origin));
        }
    }
    result
}

/// Forward bias places a character at a soft-wrap boundary on its new line.
/// GPUI's generic caret helper uses the preceding line at that exact byte.
pub(crate) fn wrapped_position(
    layout: &WrappedLineLayout,
    index: usize,
    line_height: Pixels,
    width: Pixels,
    align: TextAlign,
) -> Option<Point<Pixels>> {
    if index > layout.len() {
        return None;
    }
    let starts: Vec<_> = std::iter::once(0)
        .chain(layout.wrap_boundaries.iter().map(|boundary| {
            layout.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
        }))
        .collect();
    let row = starts
        .partition_point(|start| *start <= index)
        .saturating_sub(1);
    let start_x = layout.unwrapped_layout.x_for_index(starts[row]);
    let end_x = starts
        .get(row + 1)
        .map_or(layout.unwrapped_layout.width, |end| {
            layout.unwrapped_layout.x_for_index(*end)
        });
    let alignment = match align {
        TextAlign::Left => px(0.),
        TextAlign::Center => (width - (end_x - start_x)) / 2.,
        TextAlign::Right => width - (end_x - start_x),
    };
    Some(point(
        layout.unwrapped_layout.x_for_index(index) - start_x + alignment,
        line_height * row as f32,
    ))
}

fn text_position(
    layout: &TextLayout,
    text: &str,
    index: usize,
    align: TextAlign,
) -> Option<Point<Pixels>> {
    let start = text.get(..index)?.rfind('\n').map_or(0, |at| at + 1);
    let line = layout.line_layout_for_index(index)?;
    Some(
        layout.position_for_index(start)?
            + wrapped_position(
                &line,
                index - start,
                layout.line_height(),
                layout.bounds().size.width,
                align,
            )?,
    )
}

/// A paint-only overlay does not introduce hitboxes over InteractiveText.
pub(crate) fn reference_overlay(
    text: std::sync::Arc<str>,
    spans: Vec<Span>,
    layout: TextLayout,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let style = window.text_style();
            paint_references(
                &text,
                &spans,
                &style,
                layout.line_height(),
                |index| text_position(&layout, &text, index, style.text_align),
                window,
                cx,
            );
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::segments;
    use gpui::{point, px};

    #[test]
    fn wrapped_reference_parts_retain_original_indices() {
        let parts = segments("[12-1]", 10, &|index| {
            Some(point(
                px((index % 13) as f32),
                px((index / 13) as f32 * 24.),
            ))
        });
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, 0..3);
        assert_eq!(parts[1].0, 3..6);
        assert_eq!(parts[1].1.y, px(24.));
    }
}
