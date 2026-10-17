//! Compact source/display mappings for folds and reordered footnote sections.
use super::Callout;
use std::{collections::BTreeMap, ops::Range, sync::Arc};

mod footnotes;

#[derive(Default, Clone)]
pub struct Projection {
    /// None is identity, including out-of-range queries used during preparation.
    mapping: Option<Arc<Mapping>>,
    pub overrides: BTreeMap<usize, bool>,
}

#[derive(Debug)]
struct Mapping {
    count: usize,
    len: usize,
    runs: Vec<Run>,
    positions: Vec<Position>,
    reordered: bool,
}

#[derive(Clone, Debug)]
struct Run {
    source: Range<usize>,
    list: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Linear(usize),
    Fixed(usize),
}

#[derive(Clone, Debug)]
struct Position {
    source: Range<usize>,
    target: Target,
}

impl Projection {
    pub fn collapsed(&self, callout: &Callout) -> bool {
        callout.collapsed.is_some()
            && self
                .overrides
                .get(&callout.source.start)
                .copied()
                .unwrap_or(callout.collapsed == Some(true))
    }

    pub fn rebuild(&mut self, count: usize, callouts: &[Callout]) {
        let mut hidden: Vec<_> = callouts
            .iter()
            .filter(|callout| self.collapsed(callout))
            .map(|callout| {
                callout.rows.start.saturating_add(1).min(count)..callout.rows.end.min(count)
            })
            .filter(|range| !range.is_empty())
            .collect();
        merge(&mut hidden);
        if hidden.is_empty() {
            self.mapping = None;
            return;
        }
        let mut mapping = Mapping {
            count,
            len: 0,
            runs: Vec::new(),
            positions: Vec::new(),
            reordered: false,
        };
        let mut source = 0;
        for range in hidden {
            if source < range.start {
                mapping.visible(source..range.start);
            }
            push_position(
                &mut mapping.positions,
                range.clone(),
                Target::Fixed(mapping.len.saturating_sub(1)),
            );
            source = range.end;
        }
        if source < count {
            mapping.visible(source..count);
        }
        self.mapping = Some(Arc::new(mapping));
    }

    pub fn with_footnotes(
        &mut self,
        count: usize,
        notes: &crate::footnotes::Notes,
        callouts: &[Callout],
    ) {
        if !notes.definitions.is_empty() {
            self.mapping = Some(Arc::new(footnotes::compose(self, count, notes, callouts)));
        }
    }

    pub fn len(&self, count: usize) -> usize {
        self.mapping.as_ref().map_or(count, |mapping| mapping.len)
    }

    pub fn row(&self, list_index: usize) -> usize {
        self.mapping
            .as_ref()
            .map_or(list_index, |mapping| mapping.row(list_index))
    }

    pub fn list_index(&self, row: usize) -> usize {
        let Some(mapping) = &self.mapping else {
            return row;
        };
        if row >= mapping.count {
            // Footer projections historically return zero for invalid source
            // rows; an ordinary fold maps past-the-end to its last visible row.
            return if mapping.reordered {
                0
            } else {
                mapping.len.saturating_sub(1)
            };
        }
        position(&mapping.positions, row).unwrap_or(0)
    }
}

impl Mapping {
    fn row(&self, index: usize) -> usize {
        let at = self
            .runs
            .partition_point(|run| run.list + run.source.len() <= index);
        self.runs
            .get(at)
            .filter(|run| index >= run.list)
            .map_or(0, |run| run.source.start + index - run.list)
    }

    fn visible(&mut self, source: Range<usize>) {
        push_position(
            &mut self.positions,
            source.clone(),
            Target::Linear(self.len),
        );
        push_run(&mut self.runs, &mut self.len, source);
    }
}

fn push_run(runs: &mut Vec<Run>, len: &mut usize, source: Range<usize>) {
    if source.is_empty() {
        return;
    }
    let amount = source.len();
    if let Some(previous) = runs.last_mut()
        && previous.source.end == source.start
    {
        previous.source.end = source.end;
    } else {
        runs.push(Run { source, list: *len });
    }
    *len += amount;
}

fn push_position(positions: &mut Vec<Position>, source: Range<usize>, target: Target) {
    if source.is_empty() {
        return;
    }
    if let Some(previous) = positions.last_mut()
        && previous.source.end == source.start
        && match (&previous.target, &target) {
            (Target::Fixed(before), Target::Fixed(after)) => before == after,
            (Target::Linear(before), Target::Linear(after)) => {
                *before + previous.source.len() == *after
            }
            _ => false,
        }
    {
        previous.source.end = source.end;
    } else {
        positions.push(Position { source, target });
    }
}

fn position(positions: &[Position], row: usize) -> Option<usize> {
    let at = positions.partition_point(|position| position.source.end <= row);
    let position = positions.get(at).filter(|p| p.source.contains(&row))?;
    Some(match position.target {
        Target::Linear(index) => index + row - position.source.start,
        Target::Fixed(index) => index,
    })
}

fn merge(ranges: &mut Vec<Range<usize>>) {
    ranges.sort_unstable_by_key(|range| range.start);
    let mut write = 0;
    for read in 0..ranges.len() {
        if write > 0 && ranges[read].start <= ranges[write - 1].end {
            ranges[write - 1].end = ranges[write - 1].end.max(ranges[read].end);
        } else {
            ranges.swap(write, read);
            write += 1;
        }
    }
    ranges.truncate(write);
}

fn subtract(range: Range<usize>, excluded: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut source = range.start;
    let at = excluded.partition_point(|hidden| hidden.end <= range.start);
    for hidden in excluded[at..].iter().take_while(|h| h.start < range.end) {
        if source < hidden.start {
            result.push(source..hidden.start.min(range.end));
        }
        source = source.max(hidden.end);
    }
    if source < range.end {
        result.push(source..range.end);
    }
    result
}

#[cfg(test)]
mod tests;
