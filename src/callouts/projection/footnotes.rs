//! Reorder intervals and retain the dense projection's hidden-row anchor rules.
use super::{
    Callout, Mapping, Position, Projection, Run, Target, merge, position, push_position, push_run,
    subtract,
};
use crate::footnotes::Notes;
use std::{collections::BTreeSet, ops::Range};

pub(super) fn compose(
    previous: &Projection,
    count: usize,
    notes: &Notes,
    callouts: &[Callout],
) -> Mapping {
    let mut definitions: Vec<_> = notes
        .definitions
        .iter()
        .flat_map(|definition| definition.sections.iter())
        .filter_map(|range| clipped(range, count))
        .collect();
    merge(&mut definitions);
    let folds: Vec<_> = callouts
        .iter()
        .filter(|callout| previous.collapsed(callout) && contains(&definitions, callout.rows.start))
        .filter_map(|callout| {
            clipped(
                &(callout.rows.start.saturating_add(1)..callout.rows.end),
                count,
            )
            .map(|range| (range, callout.rows.start))
        })
        .collect();
    let folded: Vec<_> = priority(
        &folds
            .iter()
            .map(|(range, _)| range.clone())
            .collect::<Vec<_>>(),
        false,
    )
    .into_iter()
    .map(|(source, index)| (source, folds[index].1))
    .collect();
    let excluded: Vec<_> = folded.iter().map(|(range, _)| range.clone()).collect();
    let mut result = Mapping {
        count,
        len: 0,
        runs: Vec::new(),
        positions: Vec::new(),
        reordered: true,
    };
    let identity = [Run {
        source: 0..count,
        list: 0,
    }];
    let body = previous
        .mapping
        .as_ref()
        .map_or(identity.as_slice(), |mapping| mapping.runs.as_slice());
    for run in body {
        for range in subtract(run.source.clone(), &definitions) {
            push_run(&mut result.runs, &mut result.len, range);
        }
    }
    for definition in notes.definitions.iter().filter(|d| d.number > 0) {
        for range in &definition.sections {
            if let Some(range) = clipped(range, count) {
                for range in subtract(range, &excluded) {
                    push_run(&mut result.runs, &mut result.len, range);
                }
            }
        }
    }
    result.positions = positions(previous, count, &result.runs, &folded);
    result
}

fn positions(
    previous: &Projection,
    count: usize,
    runs: &[Run],
    folded: &[(Range<usize>, usize)],
) -> Vec<Position> {
    // A malformed overlapping definition still follows the legacy last-wins
    // inverse mapping; ordinary documents contain disjoint source sections.
    let visible: Vec<_> = priority(
        &runs
            .iter()
            .map(|run| run.source.clone())
            .collect::<Vec<_>>(),
        true,
    )
    .into_iter()
    .map(|(source, index)| Position {
        target: Target::Linear(runs[index].list + source.start - runs[index].source.start),
        source,
    })
    .collect();
    let old = previous.mapping.as_ref().map(|mapping| &mapping.positions);
    let mut boundaries = vec![0, count];
    boundaries.extend(visible.iter().flat_map(|p| [p.source.start, p.source.end]));
    boundaries.extend(folded.iter().flat_map(|(r, _)| [r.start, r.end]));
    if let Some(old) = old {
        boundaries.extend(old.iter().flat_map(|p| [p.source.start, p.source.end]));
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut result = Vec::new();
    for pair in boundaries.windows(2) {
        let source = pair[0]..pair[1];
        if let Some(index) = position(&visible, source.start) {
            push_position(&mut result, source, Target::Linear(index));
            continue;
        }
        let fold = folded.partition_point(|(range, _)| range.end <= source.start);
        let anchor = folded
            .get(fold)
            .filter(|(range, _)| range.contains(&source.start))
            .map_or_else(
                || previous.row(previous.list_index(source.start)),
                |(_, anchor)| *anchor,
            );
        // The old dense loop initialized visible positions first, then filled
        // hidden rows in source order. A later hidden anchor was still unset.
        let index = position(&visible, anchor)
            .or_else(|| {
                (anchor < source.start)
                    .then(|| position(&result, anchor))
                    .flatten()
            })
            .unwrap_or(0);
        push_position(&mut result, source, Target::Fixed(index));
    }
    result
}

fn clipped(range: &Range<usize>, count: usize) -> Option<Range<usize>> {
    let range = range.start.min(count)..range.end.min(count);
    (!range.is_empty()).then_some(range)
}

fn contains(ranges: &[Range<usize>], row: usize) -> bool {
    ranges
        .get(ranges.partition_point(|range| range.end <= row))
        .is_some_and(|range| range.contains(&row))
}

/// Partition overlapping intervals by their earliest/latest input occurrence.
/// Event count depends on interval count, never on document row count.
fn priority(ranges: &[Range<usize>], latest: bool) -> Vec<(Range<usize>, usize)> {
    let mut events: Vec<_> = ranges
        .iter()
        .enumerate()
        .flat_map(|(index, range)| [(range.start, index, true), (range.end, index, false)])
        .collect();
    events.sort_unstable_by_key(|event| event.0);
    let mut active = BTreeSet::new();
    let mut result = Vec::new();
    let mut cursor = 0;
    let mut at = 0;
    while at < events.len() {
        let next = events[at].0;
        if cursor < next
            && let Some(&index) = if latest {
                active.last()
            } else {
                active.first()
            }
        {
            result.push((cursor..next, index));
        }
        while at < events.len() && events[at].0 == next {
            let (_, index, start) = events[at];
            if start {
                active.insert(index);
            } else {
                active.remove(&index);
            }
            at += 1;
        }
        cursor = next;
    }
    result
}
