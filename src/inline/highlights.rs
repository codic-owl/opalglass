use super::{Mark, Output};
use std::ops::Range;

/// Overlay highlight spans after inline parsing. This preserves nested styles
/// and source mappings instead of reparsing the text inside each highlight.
pub(super) fn apply(original: Output, marks: &[Mark], eligible: &mut Vec<Range<usize>>) -> Output {
    let mut opening: Option<&Mark> = None;
    let mut highlights = vec![];
    let mut hidden = vec![];
    for mark in marks {
        if opening.is_some_and(|start| start.group != mark.group) {
            opening = None;
        }
        if let Some(start) = opening {
            if mark.close && mark.at > start.at + 2 {
                let content = start.at + 2 + start.color_bytes;
                if content <= mark.at {
                    hidden.extend([start.at..content, mark.at..mark.at + 2]);
                    highlights.push((content..mark.at, start.color));
                    opening = None;
                }
            }
        } else if mark.open {
            opening = Some(mark);
        }
    }
    if highlights.is_empty() {
        return original;
    }
    let mut cuts = vec![0, original.text.len()];
    for span in &original.spans {
        cuts.extend([span.range.start, span.range.end]);
    }
    for range in &hidden {
        cuts.extend([range.start, range.end]);
    }
    for range in eligible.iter() {
        cuts.extend([range.start, range.end]);
    }
    for (range, _) in &highlights {
        cuts.extend([range.start, range.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut output = Output {
        text: String::with_capacity(original.text.len()),
        spans: vec![],
        mapping: original.mapping.as_ref().map(|_| vec![]),
    };
    let (mut span_index, mut hidden_index, mut highlight_index) = (0, 0, 0);
    let mut link_index = 0;
    let mut linkable: Vec<Range<usize>> = Vec::new();
    for pair in cuts.windows(2) {
        let range = pair[0]..pair[1];
        while hidden_index < hidden.len() && hidden[hidden_index].end <= range.start {
            hidden_index += 1;
        }
        if hidden
            .get(hidden_index)
            .is_some_and(|r| r.contains(&range.start))
        {
            continue;
        }
        while span_index < original.spans.len()
            && original.spans[span_index].range.end <= range.start
        {
            span_index += 1;
        }
        while highlight_index < highlights.len() && highlights[highlight_index].0.end <= range.start
        {
            highlight_index += 1;
        }
        let mut style = original
            .spans
            .get(span_index)
            .filter(|s| s.range.contains(&range.start))
            .map(|s| s.style.clone())
            .unwrap_or_default();
        if let Some((_, color)) = highlights
            .get(highlight_index)
            .filter(|(r, _)| r.contains(&range.start))
        {
            style.highlight = Some(*color);
        }
        let start = output.text.len();
        output.append(&original.text[range.clone()], &style, 0..0, false);
        while link_index < eligible.len() && eligible[link_index].end <= range.start {
            link_index += 1;
        }
        if eligible
            .get(link_index)
            .is_some_and(|r| r.contains(&range.start))
        {
            if let Some(previous) = linkable.last_mut()
                && previous.end == start
            {
                previous.end = output.text.len();
            } else {
                linkable.push(start..output.text.len());
            }
        }
        if let (Some(mapping), Some(previous)) = (&mut output.mapping, &original.mapping) {
            mapping.truncate(start);
            mapping.extend_from_slice(&previous[range.start..=range.end]);
        }
    }
    *eligible = linkable;
    output
}
