//! Callout containers are indexed once; only visible lines lose quote prefixes.
use super::{Context, Kind, Line};
use crate::{callouts::Callout, rendering::Block};
use std::ops::Range;

#[derive(Debug)]
pub struct LiveCallout {
    pub callout: Callout,
    /// Complete physical title line, excluding CR/LF and including quote markers.
    pub title: Range<usize>,
    /// Number of quote containers through this callout, including plain ancestors.
    pub depth: usize,
    pub parent: Option<usize>,
    content: Range<usize>,
    default_title: String,
}

impl Context {
    pub(crate) fn with_callouts(mut self, source: &str, blocks: &[Block]) -> Self {
        let mut callouts: Vec<_> = blocks
            .iter()
            .filter_map(|block| {
                let callout = block.callout.as_ref()?;
                let (_, content, default_title) = Callout::parse(source, callout.source.clone())?;
                let title_start = source[..callout.source.start]
                    .rfind('\n')
                    .map_or(0, |at| at + 1);
                let rest = &source[title_start..];
                let title_end = title_start + rest.find('\n').unwrap_or(rest.len());
                let title_end =
                    title_start + source[title_start..title_end].trim_end_matches('\r').len();
                Some(LiveCallout {
                    callout: callout.clone(),
                    title: title_start..title_end,
                    depth: quote_depth(&source[title_start..callout.source.start]),
                    parent: None,
                    content,
                    default_title,
                })
            })
            .collect();
        callouts.sort_unstable_by_key(|c| c.title.start);
        let mut parents: Vec<usize> = Vec::new();
        for index in 0..callouts.len() {
            let current = callouts[index].title.start;
            while parents
                .last()
                .is_some_and(|&parent| callouts[parent].callout.source.end <= current)
            {
                parents.pop();
            }
            callouts[index].parent = parents.last().copied();
            parents.push(index);
        }
        self.callouts = callouts;
        self
    }

    pub fn callouts(&self) -> &[LiveCallout] {
        &self.callouts
    }

    /// Find the innermost containing callout in O(log callouts + nesting depth).
    pub fn callout_at(&self, source: usize) -> Option<usize> {
        let end = self.callouts.partition_point(|c| c.title.start <= source);
        let mut index = end.checked_sub(1)?;
        loop {
            let current = &self.callouts[index];
            if current.title.start <= source && source < current.callout.source.end {
                return Some(index);
            }
            index = current.parent?;
        }
    }

    pub(super) fn callout_masks(
        &self,
        source: &str,
        range: Range<usize>,
    ) -> (Vec<Range<usize>>, Option<usize>) {
        let Some(index) = self.callout_at(range.end.saturating_sub(1)) else {
            return (Vec::new(), None);
        };
        let callout = &self.callouts[index];
        // Continuation chunks cannot intersect a container prefix. Avoid
        // searching backward through a pathological multi-megabyte source line.
        let physical_start =
            range.start == 0 || source.as_bytes().get(range.start - 1) == Some(&b'\n');
        let prefix = if physical_start {
            quote_prefix(&source[range.clone()], callout.depth)
        } else {
            0
        };
        let mut masks = Vec::with_capacity(2);
        if prefix > 0 {
            masks.push(range.start..range.start + prefix);
        }
        let title =
            (callout.title.start < range.end && callout.title.end >= range.start).then_some(index);
        if title.is_some() {
            masks.push(callout.title.start..callout.content.start);
        }
        (masks, title)
    }

    pub(super) fn default_callout_title(
        &self,
        index: usize,
        range: Range<usize>,
        visible: &str,
    ) -> Option<Line> {
        let callout = &self.callouts[index];
        if !visible.trim().is_empty() || self.is_hidden(callout.callout.source.start) {
            return None;
        }
        // A synthesized label belongs to the title line, never a continuation
        // chunk. Its bytes map atomically to the source marker.
        let marker = callout.callout.source.start;
        if !range.contains(&marker) {
            return None;
        }
        let mut mapping = vec![marker - range.start; callout.default_title.len()];
        mapping.push(range.len());
        Some(Line {
            text: callout.default_title.clone(),
            spans: Vec::new(),
            kind: Kind::CalloutTitle,
            mapping,
            source_len: range.len(),
        })
    }
}

fn quote_prefix(line: &str, depth: usize) -> usize {
    let bytes = line.as_bytes();
    let mut end = 0;
    for _ in 0..depth {
        let mut marker = end;
        while bytes.get(marker).is_some_and(|b| matches!(b, b' ' | b'\t')) {
            marker += 1;
        }
        if bytes.get(marker) != Some(&b'>') {
            break;
        }
        end = marker + 1;
        if bytes.get(end).is_some_and(|b| matches!(b, b' ' | b'\t')) {
            end += 1;
        }
    }
    end
}

fn quote_depth(prefix: &str) -> usize {
    // Reading makes quote depth relative to a footnote definition. The source
    // projection needs the complete physical depth, including outer containers.
    let mut depth = 1;
    let mut brackets = 0_usize;
    let mut escaped = false;
    for byte in prefix.bytes() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'>' if brackets == 0 => depth += 1,
            _ => {}
        }
    }
    depth
}

#[cfg(test)]
mod tests;
