//! Document metadata shared by visible-line projections. No note body is retained.
use super::{Kind, Line};
use crate::{
    footnotes,
    markdown::{Span, WikiLink},
};
use std::{ops::Range, sync::Arc};

#[derive(Default, Debug)]
pub struct Context {
    pub(crate) hidden: Vec<Range<usize>>,
    pub(crate) links: Arc<Vec<WikiLink>>,
    pub(crate) references: Vec<footnotes::Reference>,
    pub(crate) headings: Vec<(Range<usize>, u8)>,
    definitions: Vec<definitions::Definition>,
    callouts: Vec<callouts::LiveCallout>,
}

impl Context {
    pub fn is_hidden(&self, source: usize) -> bool {
        let at = self.hidden.partition_point(|r| r.end <= source);
        self.hidden.get(at).is_some_and(|r| r.contains(&source))
    }

    pub fn hidden_ranges(&self) -> &[Range<usize>] {
        &self.hidden
    }

    pub fn link_target_at(&self, source: usize) -> Option<&str> {
        let end = self.links.partition_point(|l| l.range.start <= source);
        self.links[..end]
            .iter()
            .rev()
            .find(|l| l.range.contains(&source))
            .map(|l| l.target.as_str())
    }

    /// Editing any part of multiline hidden syntax reveals the complete source
    /// construct, so its closing delimiter remains visible while typing.
    pub fn reveal_range(&self, mut range: Range<usize>) -> Range<usize> {
        let start = self.hidden.partition_point(|r| r.end <= range.start);
        for hidden in &self.hidden[start..] {
            if hidden.start >= range.end {
                break;
            }
            range.start = range.start.min(hidden.start);
            range.end = range.end.max(hidden.end);
        }
        let start = self
            .references
            .partition_point(|r| r.source.end <= range.start);
        for reference in &self.references[start..] {
            if reference.source.start >= range.end {
                break;
            }
            range.start = range.start.min(reference.source.start);
            range.end = range.end.max(reference.source.end);
        }
        self.reveal_definitions(range)
    }

    pub(crate) fn new(
        source: &str,
        mut hidden: Vec<Range<usize>>,
        definitions: Vec<Range<usize>>,
        links: Arc<Vec<WikiLink>>,
        notes: &footnotes::Notes,
        anchors: &crate::anchors::Index,
        frontmatter: &Option<Range<usize>>,
    ) -> Self {
        if let Some(header) = frontmatter {
            // The hidden-range merger may have joined a body comment directly
            // to the metadata fence. Keep the metadata editable while retaining
            // the adjacent comment's hidden portion.
            hidden = hidden
                .into_iter()
                .flat_map(|range| {
                    let before = (range.start < header.start)
                        .then_some(range.start..range.end.min(header.start));
                    let after =
                        (range.end > header.end).then_some(range.start.max(header.end)..range.end);
                    before.into_iter().chain(after)
                })
                .collect();
        }
        hidden.extend(definitions);
        hidden.extend(anchors.markers.iter().cloned());
        let mut headings = Vec::new();
        for heading in &anchors.headings {
            let raw = &source[heading.source.clone()];
            let trimmed = raw.trim_end_matches(['\r', '\n']);
            let title_end = if let Some((title, last)) = trimmed.rsplit_once('\n')
                && setext_marker(last, heading.level)
            {
                let marker_start = heading.source.start + title.len() + 1;
                hidden.push(marker_start..heading.source.end);
                title.len()
            } else {
                trimmed.len()
            };
            let mut start = heading.source.start;
            for line in raw[..title_end].split_inclusive('\n') {
                let end = start + line.trim_end_matches(['\r', '\n']).len();
                headings.push((start..end, heading.level));
                start += line.len();
            }
        }
        hidden.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for range in hidden {
            if let Some(last) = merged.last_mut()
                && range.start <= last.end
            {
                last.end = last.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        let mut references: Vec<footnotes::Reference> = Vec::new();
        for reference in &notes.references {
            if references
                .last()
                .is_none_or(|r| r.source.end <= reference.source.start)
            {
                references.push(reference.clone());
            }
        }
        Self {
            hidden: merged,
            links,
            references,
            headings,
            definitions: Vec::new(),
            callouts: Vec::new(),
        }
    }

    pub fn project(
        &self,
        source: &str,
        range: Range<usize>,
        reveal: bool,
        code: bool,
        continuation: bool,
    ) -> Line {
        let original = &source[range.clone()];
        if reveal {
            return Line::new(original, reveal, code, continuation);
        }
        let (extra, title) = self.callout_masks(source, range.clone());
        let (visible, mapping) = self.visible(source, range.clone(), &extra, !code);
        if let Some(line) =
            title.and_then(|index| self.default_callout_title(index, range.clone(), &visible))
        {
            return self.present_definition(line, range);
        }
        let mut line = Line::parsed(
            &visible,
            false,
            code,
            continuation || title.is_some(),
            |text, skip| {
                crate::inline::parse_resolved(text, |broken| {
                    let start = skip + broken.start.saturating_sub(2);
                    let end = skip + broken.end.saturating_sub(2);
                    let original_start = range.start + *mapping.get(start)?;
                    let original_end = range.start + *mapping.get(end)?;
                    let at = self
                        .links
                        .partition_point(|l| l.range.start < original_start);
                    let link = self.links.get(at).filter(|l| {
                        l.range.start == original_start && l.range.end <= original_end
                    })?;
                    Some(link.target.clone())
                })
            },
        );
        line.mapping = (0..=line.text.len())
            .map(|at| mapping[line.source_index(at).min(visible.len())])
            .collect();
        line.source_len = range.len();
        if code {
            return line;
        }
        let heading = self.headings.partition_point(|(r, _)| r.end <= range.start);
        if let Some((_, level)) = self
            .headings
            .get(heading)
            .filter(|(r, _)| r.start < range.end)
        {
            line.kind = Kind::Heading(*level);
        }
        if title.is_some() {
            line.kind = Kind::CalloutTitle;
        }
        let line = self.number_references(line, range.clone());
        self.present_definition(line, range)
    }

    fn visible(
        &self,
        source: &str,
        range: Range<usize>,
        extra: &[Range<usize>],
        hide_document_syntax: bool,
    ) -> (String, Vec<usize>) {
        let mut text = String::new();
        let mut mapping = Vec::new();
        let mut at = range.start;
        let mut masks = extra.to_vec();
        if hide_document_syntax {
            let first = self.hidden.partition_point(|r| r.end <= range.start);
            masks.extend(
                self.hidden[first..]
                    .iter()
                    .take_while(|r| r.start < range.end)
                    .cloned(),
            );
        }
        masks.sort_unstable_by_key(|r| r.start);
        for hidden in masks {
            if hidden.end <= at || hidden.start >= range.end {
                continue;
            }
            let end = hidden.start.max(at).min(range.end);
            text.push_str(&source[at..end]);
            mapping.extend(at - range.start..end - range.start);
            at = at.max(hidden.end.min(range.end));
        }
        text.push_str(&source[at..range.end]);
        mapping.extend(at - range.start..range.len());
        mapping.push(range.len());
        (text, mapping)
    }

    fn number_references(&self, original: Line, range: Range<usize>) -> Line {
        let first = self
            .references
            .partition_point(|r| r.source.end <= range.start);
        let refs: Vec<_> = self.references[first..]
            .iter()
            .take_while(|r| r.source.start < range.end)
            .collect();
        if refs.is_empty() {
            return original;
        }
        let mut result = Line {
            text: String::new(),
            spans: vec![],
            mapping: vec![],
            kind: original.kind,
            source_len: range.len(),
        };
        let mut cursor = 0;
        for reference in refs {
            let start = original.display_index(reference.source.start.saturating_sub(range.start));
            let end = original.display_index(reference.source.end.min(range.end) - range.start);
            copy(&original, cursor..start, &mut result);
            if reference.source.start >= range.start {
                let text = reference.display();
                let at = result.text.len();
                result.text.push_str(&text);
                let mut style = original
                    .spans
                    .iter()
                    .find(|s| s.range.contains(&start))
                    .map(|s| s.style.clone())
                    .unwrap_or_default();
                style.target = Some(format!(
                    "{}{number}",
                    footnotes::TARGET_PREFIX,
                    number = reference.number
                ));
                style.image = false;
                result.spans.push(Span {
                    range: at..result.text.len(),
                    style,
                });
                result.mapping.truncate(at);
                result.mapping.extend(std::iter::repeat_n(
                    reference.source.start - range.start,
                    text.len(),
                ));
                result
                    .mapping
                    .push(reference.source.end.min(range.end) - range.start);
            }
            cursor = end;
        }
        copy(&original, cursor..original.text.len(), &mut result);
        if result.mapping.is_empty() {
            result.mapping.push(range.len());
        }
        result
    }
}

fn setext_marker(line: &str, level: u8) -> bool {
    let mut marker = line.trim();
    while let Some(rest) = marker.strip_prefix('>') {
        marker = rest.trim_start();
    }
    let expected = if level == 1 { b'=' } else { b'-' };
    !marker.is_empty() && marker.bytes().all(|b| b == expected)
}

#[cfg(test)]
mod tests;

fn copy(source: &Line, range: Range<usize>, target: &mut Line) {
    let at = target.text.len();
    target.text.push_str(&source.text[range.clone()]);
    for span in source
        .spans
        .iter()
        .filter(|s| s.range.start < range.end && s.range.end > range.start)
    {
        target.spans.push(Span {
            range: at + span.range.start.max(range.start) - range.start
                ..at + span.range.end.min(range.end) - range.start,
            style: span.style.clone(),
        });
    }
    target.mapping.truncate(at);
    target
        .mapping
        .extend((range.start..=range.end).map(|i| source.source_index(i)));
}

mod callouts;
mod definitions;
pub use callouts::LiveCallout;
