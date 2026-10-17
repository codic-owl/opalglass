//! Named footnote definitions retain source identity while showing numbered text.
use super::{Context, Line, Span};
use crate::{footnotes, markdown::InlineStyle, rendering::Block};
use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
};

#[derive(Debug)]
pub(super) struct Definition {
    pub source: Range<usize>,
    /// Prefix maximum allows binary search even when definitions are nested.
    max_end: usize,
    marker: Range<usize>,
    number: usize,
    backlink: Option<usize>,
}

impl Context {
    pub(crate) fn with_definitions(
        mut self,
        source: &str,
        blocks: &[Block],
        notes: &footnotes::Notes,
    ) -> Self {
        let by_label: HashMap<_, _> = notes
            .definitions
            .iter()
            .map(|note| (note.label.as_ref(), note))
            .collect();
        let mut definitions = BTreeMap::new();
        for block in blocks {
            let Some(range) = &block.footnote_source else {
                continue;
            };
            let Some(label) = &block.footnote else {
                continue;
            };
            let Some(note) = by_label.get(label.as_ref()) else {
                continue;
            };
            let Some(marker) = marker(source, range.clone()) else {
                continue;
            };
            definitions.entry(range.start).or_insert(Definition {
                source: range.clone(),
                max_end: 0,
                marker,
                number: note.number,
                backlink: note.references.first().copied(),
            });
        }
        self.definitions = definitions.into_values().collect();
        let mut max_end = 0;
        for definition in &mut self.definitions {
            max_end = max_end.max(definition.source.end);
            definition.max_end = max_end;
        }
        for definition in &self.definitions {
            if definition.number == 0 {
                // Referenced nested definitions remain reachable independently
                // from the parent definition that happens to contain them.
                self.hidden
                    .extend(unused_ranges(definition, &self.definitions));
            } else {
                self.hidden.push(definition.marker.clone());
            }
            let raw = &source[definition.source.clone()];
            let mut start = definition.source.start;
            for (index, line) in raw.split_inclusive('\n').enumerate() {
                // The definition's continuation indent is container syntax.
                // Nested lists/quotes retain their own remaining indentation.
                if index != 0 && line.starts_with("    ") {
                    self.hidden.push(start..start + 4);
                } else if index != 0 && line.starts_with('\t') {
                    self.hidden.push(start..start + 1);
                }
                start += line.len();
            }
        }
        self.hidden.sort_by_key(|range| range.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for range in self.hidden {
            if let Some(previous) = merged.last_mut()
                && range.start <= previous.end
            {
                previous.end = previous.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        self.hidden = merged;
        self
    }

    pub fn footnote_at(&self, source: usize) -> Option<usize> {
        let end = self
            .definitions
            .partition_point(|d| d.source.start <= source);
        let first = self.definitions[..end].partition_point(|d| d.max_end <= source);
        self.definitions[first..end]
            .iter()
            .rev()
            .find(|d| d.source.contains(&source))
            .map(|d| d.number)
    }

    /// Only the marker row owns the synthetic number. Hidden comment and
    /// continuation rows inside a referenced definition can still be compacted.
    pub fn footnote_prefix_in(&self, range: Range<usize>) -> bool {
        let first = self
            .definitions
            .partition_point(|definition| definition.marker.start < range.start);
        self.definitions[first..]
            .iter()
            .take_while(|definition| {
                definition.marker.start < range.end
                    || (range.start == range.end && definition.marker.start == range.start)
            })
            .any(|definition| definition.number > 0)
    }

    pub(super) fn reveal_definitions(&self, mut range: Range<usize>) -> Range<usize> {
        let first = self
            .definitions
            .partition_point(|d| d.max_end <= range.start);
        for definition in &self.definitions[first..] {
            if definition.source.start >= range.end {
                break;
            }
            range.start = range.start.min(definition.source.start);
            range.end = range.end.max(definition.source.end);
        }
        range
    }

    pub(super) fn present_definition(&self, original: Line, range: Range<usize>) -> Line {
        let first = self
            .definitions
            .partition_point(|d| d.marker.start < range.start);
        let Some(definition) = self
            .definitions
            .get(first)
            .filter(|d| d.number > 0 && d.marker.start < range.end)
        else {
            return original;
        };
        let prefix = format!("{}. ", definition.number);
        let mut result = Line {
            text: prefix.clone(),
            spans: vec![Span {
                range: 0..prefix.len(),
                style: InlineStyle {
                    target: definition
                        .backlink
                        .map(|at| format!("{}{at}", footnotes::BACK_PREFIX)),
                    ..Default::default()
                },
            }],
            kind: original.kind,
            source_len: range.len(),
            mapping: vec![definition.marker.start - range.start; prefix.len()],
        };
        super::copy(&original, 0..original.text.len(), &mut result);
        result
    }
}

fn unused_ranges(definition: &Definition, definitions: &[Definition]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = definition.source.start;
    let first = definitions.partition_point(|d| d.source.start <= definition.source.start);
    for nested in definitions[first..]
        .iter()
        .take_while(|d| d.source.start < definition.source.end)
    {
        if nested.number == 0 {
            continue;
        }
        if cursor < nested.source.start {
            ranges.push(cursor..nested.source.start);
        }
        cursor = cursor.max(nested.source.end);
    }
    if cursor < definition.source.end {
        ranges.push(cursor..definition.source.end);
    }
    ranges
}

fn marker(source: &str, range: Range<usize>) -> Option<Range<usize>> {
    let line = source.get(range.clone())?.split('\n').next()?;
    let start = line.find("[^")?;
    let bytes = line.as_bytes();
    let mut end = start + 2;
    while end < bytes.len() {
        match bytes[end] {
            b'\\' => end += 2,
            b']' if bytes.get(end + 1) == Some(&b':') => {
                end += 2;
                while bytes.get(end).is_some_and(|b| matches!(b, b' ' | b'\t')) {
                    end += 1;
                }
                return Some(range.start + start..range.start + end);
            }
            _ => end += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests;
