//! Select prepared row intervals and referenced footers without walking text rows.
use opalglass::{callouts::Projection, embeds::Selection, prepared::Prepared};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
};

pub(super) struct Rows {
    /// Physical prepared ranges used to seek the background occurrence index.
    pub(super) physical: Vec<Range<usize>>,
    /// Display intervals in document-projection order, including selected footers.
    pub(super) logical: Vec<Range<usize>>,
}

pub(super) fn select(selection: &Selection, projection: &Projection, whole: bool) -> Rows {
    let prepared = &selection.prepared;
    if whole {
        return Rows {
            physical: std::iter::once(0..prepared.rows.len()).collect(),
            logical: std::iter::once(0..projection.len(prepared.rows.len())).collect(),
        };
    }
    let definitions = &prepared.footnotes.definitions;
    let mut excluded: Vec<_> = definitions
        .iter()
        .flat_map(|d| d.sections.iter().cloned())
        .collect();
    merge(&mut excluded);
    let mut physical = subtract(selection.rows.clone(), &excluded);
    let mut logical = physical
        .iter()
        .filter_map(|range| projected(projection, prepared.rows.len(), range.clone()))
        .collect::<Vec<_>>();
    let referenced = referenced_definitions(prepared, &selection.source);
    for definition in definitions
        .iter()
        .filter(|d| d.number > 0 && referenced.contains(&d.number))
    {
        for range in &definition.sections {
            if let Some(display) = projected(projection, prepared.rows.len(), range.clone()) {
                physical.push(range.clone());
                logical.push(display);
            }
        }
    }
    // Adjacent source sections may share a single display run. Keep metadata
    // intervals separate only when the parser's source order requires it.
    merge_ordered(&mut logical);
    merge(&mut physical);
    Rows { physical, logical }
}

fn referenced_definitions(prepared: &Prepared, selected: &Range<usize>) -> HashSet<usize> {
    let notes = &prepared.footnotes;
    let by_number = notes
        .definitions
        .iter()
        .map(|d| (d.number, d))
        .collect::<HashMap<_, _>>();
    let mut needed = HashSet::new();
    let mut queue = VecDeque::new();
    for reference in &notes.references {
        if reference.source.start < selected.end
            && reference.source.end > selected.start
            && needed.insert(reference.number)
        {
            queue.push_back(reference.number);
        }
    }
    while let Some(number) = queue.pop_front() {
        let Some(definition) = by_number.get(&number) else {
            continue;
        };
        for section in &definition.sections {
            let Some(first) = prepared.rows.get(section.start) else {
                continue;
            };
            let Some(last) = section
                .end
                .checked_sub(1)
                .and_then(|row| prepared.rows.get(row))
            else {
                continue;
            };
            let source = first.source.start..last.source.end;
            let at = notes
                .references
                .partition_point(|r| r.source.start < source.start);
            for reference in notes.references[at..]
                .iter()
                .take_while(|r| r.source.start < source.end)
            {
                if needed.insert(reference.number) {
                    queue.push_back(reference.number);
                }
            }
        }
    }
    needed
}

/// Callout folds map hidden rows to their preceding title. A single boundary
/// adjustment skips that title if it lies outside the requested source section.
fn projected(projection: &Projection, count: usize, source: Range<usize>) -> Option<Range<usize>> {
    if source.is_empty() || projection.len(count) == 0 {
        return None;
    }
    let mut start = projection.list_index(source.start);
    if projection.row(start) < source.start {
        start += 1;
    }
    if start >= projection.len(count) || !source.contains(&projection.row(start)) {
        return None;
    }
    let last = projection.list_index(source.end - 1);
    (last >= start && source.contains(&projection.row(last))).then_some(start..last + 1)
}

fn subtract(range: Range<usize>, excluded: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut start = range.start;
    let at = excluded.partition_point(|r| r.end <= range.start);
    for hidden in excluded[at..].iter().take_while(|r| r.start < range.end) {
        if start < hidden.start {
            result.push(start..hidden.start.min(range.end));
        }
        start = start.max(hidden.end);
    }
    if start < range.end {
        result.push(start..range.end);
    }
    result
}

fn merge(ranges: &mut Vec<Range<usize>>) {
    ranges.sort_unstable_by_key(|r| r.start);
    merge_ordered(ranges);
}

fn merge_ordered(ranges: &mut Vec<Range<usize>>) {
    let mut write = 0;
    for read in 0..ranges.len() {
        if ranges[read].is_empty() {
            continue;
        }
        if write > 0 && ranges[read].start <= ranges[write - 1].end {
            ranges[write - 1].end = ranges[write - 1].end.max(ranges[read].end);
        } else {
            ranges.swap(write, read);
            write += 1;
        }
    }
    ranges.truncate(write);
}

#[cfg(test)]
#[path = "compose_selection/tests.rs"]
mod tests;
