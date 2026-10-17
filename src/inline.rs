//! CommonMark inline parsing with source-byte mappings for native editing.
use crate::markdown::{HighlightColor, InlineStyle, Span, TAG_TARGET_PREFIX};
use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};
use std::ops::Range;

struct Output {
    text: String,
    spans: Vec<Span>,
    mapping: Option<Vec<usize>>,
}
impl Output {
    fn append(&mut self, text: &str, style: &InlineStyle, range: Range<usize>, exact: bool) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        if let Some(mapping) = &mut self.mapping {
            mapping.truncate(start);
            if exact {
                mapping.extend(range.start..range.start + text.len());
            } else {
                mapping.extend(std::iter::repeat_n(range.start, text.len()));
            }
            mapping.push(range.end);
        }
        if *style != InlineStyle::default() {
            if let Some(previous) = self.spans.last_mut()
                && previous.range.end == start
                && previous.style == *style
            {
                previous.range.end = self.text.len();
            } else {
                self.spans.push(Span {
                    range: start..self.text.len(),
                    style: style.clone(),
                });
            }
        }
    }
    fn text(&mut self, text: &str, style: &InlineStyle, range: Range<usize>, exact: bool) {
        if style.code || style.target.is_some() || !exact {
            self.append(text, style, range, exact);
            return;
        }
        let mut start = 0;
        for tag_range in crate::markdown::text_tags(text, self.text.chars().next_back()) {
            let index = tag_range.start;
            let end = tag_range.end;
            let tag = &text[index + 1..end];
            self.append(
                &text[start..index],
                style,
                range.start + start..range.start + index,
                true,
            );
            let mut tag_style = style.clone();
            tag_style.target = Some(format!("{TAG_TARGET_PREFIX}{tag}"));
            self.append(
                &text[index..end],
                &tag_style,
                range.start + index..range.start + end,
                true,
            );
            start = end;
        }
        self.append(&text[start..], style, range.start + start..range.end, true);
    }
}

/// Consumes inline events from either a complete document or a single editor
/// line. Whole-document parsing retains reference definitions across blocks.
pub(crate) struct Builder {
    output: Output,
    style: InlineStyle,
    stack: Vec<(InlineStyle, bool, usize)>,
    wiki: bool,
    marks: Vec<Mark>,
    group: usize,
    linkable: Vec<Range<usize>>,
}
struct Mark {
    group: usize,
    at: usize,
    open: bool,
    close: bool,
    color: HighlightColor,
    color_bytes: usize,
}
impl Builder {
    pub(crate) fn new(mapped: bool) -> Self {
        Self {
            output: Output {
                text: String::new(),
                spans: Vec::new(),
                mapping: mapped.then(Vec::new),
            },
            style: InlineStyle::default(),
            stack: Vec::new(),
            wiki: false,
            marks: vec![],
            group: 0,
            linkable: vec![],
        }
    }
    pub(crate) fn literal(&mut self, text: &str) {
        self.output.append(text, &self.style, 0..0, false);
    }
    pub(crate) fn event(
        &mut self,
        event: Event<'_>,
        event_range: Range<usize>,
        input: &str,
        offset: usize,
    ) {
        match event {
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::TableCell) => {
                self.group += 1;
            }
            Event::Start(tag) => {
                let mut next = self.style.clone();
                let mut wiki = self.wiki;
                match tag {
                    Tag::Emphasis => next.italic = true,
                    Tag::Strong => next.bold = true,
                    Tag::Strikethrough => next.strike = true,
                    Tag::Link {
                        dest_url,
                        link_type,
                        ..
                    }
                    | Tag::Image {
                        dest_url,
                        link_type,
                        ..
                    } => {
                        next.image = input[event_range.clone()].starts_with('!');
                        next.image_start = if next.image { event_range.start } else { 0 };
                        next.image_dimensions = None;
                        wiki = matches!(link_type, LinkType::WikiLink { .. });
                        // Tables require an escaped pipe before a wiki alias.
                        // pulldown-cmark leaves that escape on the destination.
                        next.target = Some(if wiki && input[event_range.clone()].contains("\\|") {
                            dest_url
                                .strip_suffix('\\')
                                .unwrap_or(&dest_url)
                                .trim()
                                .to_owned()
                        } else {
                            dest_url.into_string()
                        });
                    }
                    _ => return,
                }
                self.stack
                    .push((self.style.clone(), self.wiki, self.output.text.len()));
                self.style = next;
                self.wiki = wiki;
            }
            Event::End(
                end @ (TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image),
            ) => {
                let (style, wiki, start) = self.stack.pop().unwrap_or_default();
                if self.style.image && self.output.text.len() == start {
                    let target = self.style.target.clone().unwrap_or_default();
                    self.output.append(
                        &target,
                        &self.style,
                        event_range.start.saturating_sub(offset)
                            ..event_range.end.saturating_sub(offset),
                        false,
                    );
                }
                if end == TagEnd::Image {
                    let dimensions =
                        crate::images::Dimensions::from_label(&self.output.text[start..]);
                    for span in self.output.spans.iter_mut().rev() {
                        if span.range.end <= start {
                            break;
                        }
                        if span.style.image && span.style.image_start == self.style.image_start {
                            span.style.image_dimensions = dimensions;
                        }
                    }
                }
                self.style = style;
                self.wiki = wiki;
            }
            Event::Text(text) => self.value(&text, event_range, input, offset, false, true),
            Event::Code(text) => self.value(&text, event_range, input, offset, true, false),
            Event::Html(text) | Event::InlineHtml(text) => {
                if !text.trim_start().starts_with("<!--") {
                    self.value(&text, event_range, input, offset, false, false);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                self.output.append(
                    "\n",
                    &self.style,
                    event_range.start.saturating_sub(offset)
                        ..event_range.end.saturating_sub(offset),
                    false,
                );
            }
            Event::FootnoteReference(label) => {
                // Per-line Live Preview has no document-wide definition context.
                // Keep the editable source instead of inventing a note target.
                if self.output.mapping.is_some() {
                    self.output.append(
                        &input[event_range.clone()],
                        &self.style,
                        event_range.start.saturating_sub(offset)
                            ..event_range.end.saturating_sub(offset),
                        true,
                    );
                    return;
                }
                let mut style = self.style.clone();
                style.target = Some(format!(
                    "{}{}:{}:{label}",
                    crate::footnotes::RAW_PREFIX,
                    event_range.start,
                    event_range.end,
                ));
                self.output
                    .append(&format!("[{label}]"), &style, event_range, false);
            }
            _ => (),
        }
    }
    fn value(
        &mut self,
        text: &str,
        event_range: Range<usize>,
        input: &str,
        offset: usize,
        code: bool,
        syntax: bool,
    ) {
        if event_range.end <= offset {
            return;
        }
        let mut value = if self.wiki { text.trim() } else { text };
        let base = input.as_ptr() as usize;
        let ptr = value.as_ptr() as usize;
        let borrowed = ptr >= base && ptr + value.len() <= base + input.len();
        let mut range = if borrowed {
            ptr - base..ptr - base + value.len()
        } else {
            event_range
        };
        if range.start < offset {
            let skip = (offset - range.start).min(value.len());
            value = &value[skip..];
            range.start += skip;
        }
        let mut style = self.style.clone();
        style.code |= code;
        let escaped_colon = value.starts_with(':')
            && (input[..range.start]
                .bytes()
                .rev()
                .take_while(|b| *b == b'\\')
                .count()
                % 2
                == 1
                || input[range.clone()].starts_with("\\:"));
        if syntax && borrowed && !style.image && !self.wiki {
            for (at, _) in value.match_indices("==") {
                let source_at = range.start + at;
                if input[..source_at]
                    .bytes()
                    .rev()
                    .take_while(|b| *b == b'\\')
                    .count()
                    % 2
                    == 1
                {
                    continue;
                }
                let after = &input[source_at + 2..];
                let (color, color_bytes) =
                    HighlightColor::prefix(after).unwrap_or((HighlightColor::Default, 0));
                self.marks.push(Mark {
                    group: self.group,
                    at: self.output.text.len() + at,
                    open: after.chars().next().is_some_and(|c| !c.is_whitespace()),
                    close: input[..source_at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| !c.is_whitespace()),
                    color,
                    color_bytes,
                });
            }
        }
        let start = self.output.text.len();
        self.output.text(
            value,
            &style,
            range.start.saturating_sub(offset)..range.end.saturating_sub(offset),
            borrowed,
        );
        if syntax && !code && style.target.is_none() && !style.image {
            let start = start + usize::from(escaped_colon);
            let end = self.output.text.len();
            if let Some(previous) = self.linkable.last_mut()
                && previous.end == start
            {
                previous.end = end;
            } else if start < end {
                self.linkable.push(start..end);
            }
        }
    }
    pub(crate) fn finish(mut self, source_len: usize) -> (String, Vec<Span>, Vec<usize>) {
        let mut output = highlights::apply(self.output, &self.marks, &mut self.linkable);
        autolinks::apply(&mut output, &self.linkable);
        let mapping = output
            .mapping
            .map(|mut mapping| {
                if mapping.is_empty() {
                    mapping.push(source_len);
                }
                mapping
            })
            .unwrap_or_default();
        (output.text, output.spans, mapping)
    }
}

pub fn parse(source: &str, mapped: bool) -> (String, Vec<Span>, Vec<usize>) {
    // A neutral paragraph prefix prevents block interpretation of an editor line.
    let prefixed = format!("x {source}");
    let visible = crate::links::visible_source(&prefixed);
    let input = &visible.text;
    let mut builder = Builder::new(mapped);
    for (event, range) in Parser::new_ext(input, crate::links::options()).into_offset_iter() {
        builder.event(event, range, input, 2);
    }
    let (text, spans, mut mapping) = builder.finish(source.len());
    if mapped {
        for position in &mut mapping {
            *position = visible
                .original_offset(*position + 2)
                .saturating_sub(2)
                .min(source.len());
        }
    }
    (text, spans, mapping)
}

/// The caller already removed hidden document syntax and supplies reference
/// destinations from the whole-document parse, without appending definitions.
pub(crate) fn parse_resolved(
    source: &str,
    mut resolve: impl FnMut(Range<usize>) -> Option<String>,
) -> (String, Vec<Span>, Vec<usize>) {
    let input = format!("x {source}");
    let mut builder = Builder::new(true);
    let mut callback = |broken: pulldown_cmark::BrokenLink<'_>| {
        resolve(broken.span).map(|target| (target.into(), "".into()))
    };
    for (event, range) in
        Parser::new_with_broken_link_callback(&input, crate::links::options(), Some(&mut callback))
            .into_offset_iter()
    {
        builder.event(event, range, &input, 2);
    }
    builder.finish(source.len())
}

mod autolinks;
mod highlights;

#[cfg(test)]
mod tests;
