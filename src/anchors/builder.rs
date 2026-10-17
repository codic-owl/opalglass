use super::{Block, Heading, Index};
use crate::links::VisibleSource;
use pulldown_cmark::{Event, Tag, TagEnd};
use std::ops::Range;

#[derive(Default)]
pub(crate) struct Builder {
    index: Index,
    heading: Option<(Heading, crate::inline::Builder)>,
    containers: Vec<(TagEnd, Range<usize>)>,
    previous: Option<Range<usize>>,
}

impl Builder {
    pub(crate) fn event(&mut self, event: &Event<'_>, range: Range<usize>, input: &str) {
        if let Event::Start(tag) = event {
            if matches!(
                tag,
                Tag::Paragraph
                    | Tag::Heading { .. }
                    | Tag::Item
                    | Tag::List(_)
                    | Tag::BlockQuote(_)
                    | Tag::Table(_)
                    | Tag::CodeBlock(_)
            ) {
                self.containers.push((tag.to_end(), range.clone()));
            }
            if let Tag::Heading { level, .. } = tag {
                self.heading = Some((
                    Heading {
                        title: String::new(),
                        level: *level as u8,
                        source: range.clone(),
                        section: range.clone(),
                    },
                    crate::inline::Builder::new(false),
                ));
            }
        }
        if let Some((_, builder)) = &mut self.heading {
            builder.event(event.clone(), range.clone(), input, 0);
        }
        if matches!(event, Event::End(TagEnd::Heading(_)))
            && let Some((mut heading, builder)) = self.heading.take()
        {
            heading.title = builder.finish(input.len()).0;
            self.index.headings.push(heading);
        }
        if let Event::Text(_) = event {
            self.block_marker(range, input);
        }
        if let Event::End(end) = event
            && self.containers.last().is_some_and(|(tag, _)| tag == end)
            && let Some((_, range)) = self.containers.pop()
            && self.containers.is_empty()
        {
            self.previous = Some(range);
        }
    }

    fn block_marker(&mut self, text: Range<usize>, input: &str) {
        // Markers must finish a paragraph. Code/HTML/link contents, quotation
        // internals and table cells are not block identifiers.
        let Some((kind @ (TagEnd::Paragraph | TagEnd::Item), paragraph)) = self.containers.last()
        else {
            return;
        };
        if self
            .containers
            .iter()
            .any(|(tag, _)| matches!(tag, TagEnd::BlockQuote(_) | TagEnd::Table))
        {
            return;
        }
        let end = if *kind == TagEnd::Item {
            input[text.end..paragraph.end]
                .find('\n')
                .map_or(paragraph.end, |n| text.end + n)
        } else {
            paragraph.end
        };
        let raw_text = input[text.clone()].trim_end();
        let token = raw_text.rsplit(char::is_whitespace).next().unwrap_or("");
        let Some(id) = token.strip_prefix('^') else {
            return;
        };
        let at = text.start + raw_text.len() - token.len();
        let raw = input[paragraph.start..end].trim_end();
        let caret = at - paragraph.start;
        if id.is_empty()
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || at + token.len() != paragraph.start + raw.len()
            || (caret > 0
                && !raw[..caret]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace))
        {
            return;
        }
        let standalone = *kind == TagEnd::Paragraph && raw[..caret].trim().is_empty();
        let source = if standalone {
            // A standalone marker belongs to the preceding complete block,
            // including lists, callouts, quotations, tables and fenced code.
            let Some(previous) = &self.previous else {
                return;
            };
            previous.clone()
        } else {
            self.containers
                .iter()
                .rev()
                .find(|(tag, _)| matches!(tag, TagEnd::Item))
                .map_or_else(|| paragraph.clone(), |(_, range)| range.clone())
        };
        let marker = at..at + id.len() + 1;
        self.index.markers.push(marker.clone());
        self.index
            .blocks
            .entry(id.to_owned())
            .or_insert(Block { source, marker });
    }

    pub(crate) fn finish(mut self, visible: &VisibleSource<'_>, len: usize) -> Index {
        let mut ancestors: Vec<usize> = Vec::new();
        for i in 0..self.index.headings.len() {
            let level = self.index.headings[i].level;
            let start = self.index.headings[i].source.start;
            while ancestors
                .last()
                .is_some_and(|&j| self.index.headings[j].level >= level)
            {
                let j = ancestors.pop().unwrap();
                self.index.headings[j].section.end = start;
            }
            self.index.headings[i].section.end = visible.text.len();
            ancestors.push(i);
        }
        let translate = |range: &mut Range<usize>| {
            *range = visible.original_offset(range.start).min(len)
                ..visible.original_offset(range.end).min(len);
        };
        for heading in &mut self.index.headings {
            translate(&mut heading.source);
            translate(&mut heading.section);
        }
        for (i, heading) in self.index.headings.iter().enumerate() {
            let name = super::normalized(&heading.title);
            let slug = super::slug(&name);
            if slug != name {
                self.index.heading_names.entry(slug).or_default().push(i);
            }
            self.index.heading_names.entry(name).or_default().push(i);
        }
        for block in self.index.blocks.values_mut() {
            translate(&mut block.source);
            translate(&mut block.marker);
        }
        for marker in &mut self.index.markers {
            translate(marker);
        }
        self.index
    }
}
