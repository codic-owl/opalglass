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
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image,
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
                    "{}{}:{label}",
                    crate::footnotes::RAW_PREFIX,
                    event_range.start
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
        self.output.text(
            value,
            &style,
            range.start.saturating_sub(offset)..range.end.saturating_sub(offset),
            borrowed,
        );
    }
    pub(crate) fn finish(self, source_len: usize) -> (String, Vec<Span>, Vec<usize>) {
        let output = highlighted(self.output, &self.marks);
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

/// Overlay highlight spans after inline parsing. This preserves nested styles
/// and source mappings instead of reparsing the text inside each highlight.
fn highlighted(original: Output, marks: &[Mark]) -> Output {
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
        if let (Some(mapping), Some(previous)) = (&mut output.mapping, &original.mapping) {
            mapping.truncate(start);
            mapping.extend_from_slice(&previous[range.start..=range.end]);
        }
    }
    output
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn highlights_preserve_nested_styles_colors_code_and_edit_positions() {
        let source = "==outer **bold** and `code`== [==link==](Target.md) ==🔴é👩‍💻==";
        let (text, spans, mapping) = parse(source, true);
        assert_eq!(text, "outer bold and code link é👩‍💻");
        assert_eq!(mapping.len(), text.len() + 1);
        assert!(mapping.windows(2).all(|p| p[0] <= p[1]));
        for (at, _) in text.char_indices() {
            assert!(source.is_char_boundary(mapping[at]));
        }
        assert!(spans.iter().any(|s| s.style.bold
            && s.style.highlight == Some(HighlightColor::Default)
            && &text[s.range.clone()] == "bold"));
        assert!(spans.iter().any(|s| s.style.code
            && s.style.highlight.is_some()
            && &text[s.range.clone()] == "code"));
        assert!(
            spans
                .iter()
                .any(|s| s.style.target.as_deref() == Some("Target.md")
                    && s.style.highlight.is_some())
        );
        let red = spans
            .iter()
            .find(|s| s.style.highlight == Some(HighlightColor::Red))
            .unwrap();
        assert_eq!(&text[red.range.clone()], "é👩‍💻");
        assert_eq!(mapping[red.range.start], source.find('é').unwrap());
        let mut edited = source.to_owned();
        let start = text.find("bold").unwrap();
        edited.replace_range(mapping[start]..mapping[start] + 4, "strong");
        assert!(edited.contains("**strong**"));
        for literal in [
            r"\==escaped==",
            "==unclosed",
            "== spaced ==",
            "`==literal==`",
        ] {
            assert!(
                parse(literal, true)
                    .1
                    .iter()
                    .all(|s| s.style.highlight.is_none())
            );
        }
        let (text, spans, _) = parse(
            "==marked== ==🔴red== ==🟠orange== ==🟡yellow== ==🟢green== ==🔵blue== ==🟣purple==",
            false,
        );
        assert_eq!(text, "marked red orange yellow green blue purple");
        assert_eq!(spans.len(), 7);
        assert_eq!(parse("==across\nlines==", false).0, "across\nlines");
        assert_eq!(parse("[[Target|==alias==]]", false).0, "==alias==");
        assert_eq!(parse("==🔴==", false).0, "");
        assert!(parse("==first\n\nsecond==", false).1.is_empty());
        let (text, spans, _) = parse("==#work==", false);
        assert_eq!(text, "#work");
        assert_eq!(spans[0].style.target.as_deref(), Some("opalglass-tag:work"));
    }
    #[test]
    fn commonmark_delimiters_entities_links_and_code_preserve_editable_offsets() {
        let source = "***both*** snake_case_name &amp; &#233; [a **label**](<Some (note).md> \"title\") ``a ` b``";
        let (text, spans, mapping) = parse(source, true);
        assert_eq!(text, "both snake_case_name & é a label a ` b");
        assert_eq!(mapping.len(), text.len() + 1);
        assert!(mapping.windows(2).all(|p| p[0] <= p[1]));
        assert!(mapping.iter().all(|p| *p <= source.len()));
        assert!(
            spans
                .iter()
                .any(|s| s.style.bold && s.style.italic && &text[s.range.clone()] == "both")
        );
        let label = spans
            .iter()
            .find(|s| s.style.bold && s.style.target.is_some())
            .unwrap();
        assert_eq!(label.style.target.as_deref(), Some("Some (note).md"));
        assert_eq!(&source[mapping[label.range.start]..][..5], "label");
        let code = spans.iter().find(|s| s.style.code).unwrap();
        assert_eq!(&text[code.range.clone()], "a ` b");
    }
    #[test]
    fn comments_keep_unicode_edit_positions_and_empty_image_labels_have_targets() {
        let source = "%%hidden 👩‍💻%%é **word** `%%literal%%`";
        let (text, _, mapping) = parse(source, true);
        assert_eq!(text, "é word %%literal%%");
        assert_eq!(mapping[0], source.find('é').unwrap());
        assert_eq!(
            mapping[text.find("word").unwrap()],
            source.find("word").unwrap()
        );
        assert!(mapping.windows(2).all(|p| p[0] <= p[1]));
        let (text, spans, _) = parse("before ![](Image.png) after", false);
        assert_eq!(text, "before Image.png after");
        assert!(
            spans
                .iter()
                .any(|s| s.style.image && s.style.target.as_deref() == Some("Image.png"))
        );
        let links = crate::links::references("%%hidden%%[[Target|alias]]");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].destination, 12..18);
        assert!(
            crate::markdown::wiki_links("%%[[hidden]]%% [[Shown]]")
                .iter()
                .all(|l| l.target == "Shown")
        );
    }
}
