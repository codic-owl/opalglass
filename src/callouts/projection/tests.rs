use super::Projection;
use crate::{
    callouts::Callout,
    footnotes::{Definition, Notes},
};
use std::{collections::BTreeMap, ops::Range, sync::Arc};

fn callout(rows: Range<usize>, collapsed: Option<bool>) -> Callout {
    Callout {
        source: rows.start..rows.end,
        rows,
        name: "note".into(),
        collapsed,
    }
}

fn definition(label: usize, number: usize, sections: Vec<Range<usize>>) -> Definition {
    Definition {
        label: label.to_string().into(),
        number,
        rows: sections.first().cloned().unwrap_or(0..0),
        sections,
        references: Vec::new(),
    }
}

fn notes(definitions: Vec<Definition>) -> Notes {
    let mut notes = Notes::default();
    notes.definitions.extend(definitions);
    notes
}

#[test]
fn billion_row_documents_use_only_interval_metadata() {
    let count = 1_000_000_000;
    let mut projection = Projection::default();
    projection.rebuild(count, &[]);
    assert!(projection.mapping.is_none());
    assert_eq!(projection.row(count + 1), count + 1);
    let callouts = [callout(10..count - 10, Some(true))];
    projection.rebuild(count, &callouts);
    assert_eq!(projection.len(count), 21);
    assert_eq!(projection.row(10), 10);
    assert_eq!(projection.row(11), count - 10);
    assert_eq!(projection.list_index(count / 2), 10);
    let mapping = projection.mapping.as_ref().unwrap();
    assert_eq!(mapping.runs.len(), 2);
    assert_eq!(mapping.positions.len(), 3);
    let clone = projection.clone();
    assert!(Arc::ptr_eq(mapping, clone.mapping.as_ref().unwrap()));

    let notes = notes(vec![
        definition(1, 1, std::iter::once(count - 10..count).collect()),
        definition(2, 2, std::iter::once(10..20).collect()),
    ]);
    projection.rebuild(count, &[]);
    projection.with_footnotes(count, &notes, &[]);
    assert_eq!(projection.len(count), count);
    assert_eq!(projection.row(count - 20), count - 10);
    assert_eq!(projection.row(count - 10), 10);
    assert_eq!(projection.list_index(10), count - 10);
    assert_eq!(projection.list_index(count - 10), count - 20);
    let mapping = projection.mapping.as_ref().unwrap();
    assert!(mapping.runs.len() <= 4);
    assert!(mapping.positions.len() <= 4);
}

#[test]
fn billion_row_footnote_fold_retains_its_reordered_title_anchor() {
    let count = 1_000_000_000;
    let callouts = [callout(12..count - 12, Some(true))];
    let notes = notes(vec![definition(
        1,
        1,
        std::iter::once(10..count - 10).collect(),
    )]);
    let mut projection = Projection::default();
    projection.rebuild(count, &callouts);
    projection.with_footnotes(count, &notes, &callouts);
    assert_eq!(projection.len(count), 25);
    assert_eq!(projection.row(22), 12);
    assert_eq!(projection.list_index(count / 2), 22);
    assert_eq!(projection.row(23), count - 12);
    let mapping = projection.mapping.as_ref().unwrap();
    assert!(mapping.runs.len() <= 4);
    assert!(mapping.positions.len() <= 7);
}

#[test]
fn deeply_nested_folds_keep_the_outer_title_after_footer_reordering() {
    let count = 1_000_000_000;
    let callouts: Vec<_> = (10..10_010)
        .map(|start| callout(start..count - start, Some(true)))
        .collect();
    let notes = notes(vec![definition(
        1,
        1,
        std::iter::once(10..count - 10).collect(),
    )]);
    let mut projection = Projection::default();
    projection.rebuild(count, &callouts);
    assert_eq!(projection.len(count), 21);
    assert_eq!(projection.list_index(count / 2), 10);
    projection.with_footnotes(count, &notes, &callouts);
    assert_eq!(projection.len(count), 21);
    assert_eq!(projection.row(20), 10);
    assert_eq!(projection.list_index(count / 2), 20);
    let mapping = projection.mapping.as_ref().unwrap();
    assert_eq!(mapping.runs.len(), 3);
    assert_eq!(mapping.positions.len(), 4);
}

#[test]
fn footer_reordering_preserves_hidden_and_detached_definition_anchors() {
    let cases = [
        (
            vec![callout(0..10, Some(true))],
            notes(vec![definition(1, 1, std::iter::once(2..5).collect())]),
        ),
        (
            vec![callout(2..10, Some(true)), callout(3..6, Some(true))],
            notes(vec![
                definition(1, 0, vec![0..4, 5..11]),
                definition(2, 1, std::iter::once(4..5).collect()),
            ]),
        ),
        (
            vec![callout(2..11, Some(false)), callout(4..9, Some(true))],
            notes(vec![
                definition(1, 1, vec![12..14, 17..20]),
                definition(2, 2, std::iter::once(2..11).collect()),
            ]),
        ),
    ];
    for (callouts, notes) in cases {
        let mut actual = Projection::default();
        let mut expected = Dense::default();
        actual.rebuild(20, &callouts);
        expected.rebuild(20, &callouts);
        actual.with_footnotes(20, &notes, &callouts);
        expected.with_footnotes(20, &notes, &callouts);
        compare(&actual, &expected, 20);
    }
}

#[test]
fn generated_nested_folds_and_footers_match_the_original_dense_projection() {
    let mut seed = 0x92da_04f9_61b5_8ec3_u64;
    let mut next = |limit: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 32) as usize % limit
    };
    for _ in 0..600 {
        let count = next(97);
        let mut callouts = Vec::new();
        for _ in 0..next(18) {
            if count == 0 {
                break;
            }
            let start = next(count);
            let end = start + 1 + next(count - start);
            let collapsed = (next(4) != 0).then(|| next(2) == 0);
            callouts.push(callout(start..end, collapsed));
        }
        callouts.sort_by_key(|callout| (callout.rows.start, usize::MAX - callout.rows.end));
        let groups = next(6);
        let mut sections = vec![Vec::<Range<usize>>::new(); groups];
        let mut cursor = 0;
        while cursor < count {
            let end = (cursor + 1 + next(8)).min(count);
            let group = next(groups + 1);
            if group < groups {
                sections[group].push(cursor..end);
            }
            cursor = end;
        }
        let mut definitions: Vec<_> = sections
            .into_iter()
            .enumerate()
            .map(|(group, sections)| {
                definition(group, if next(4) == 0 { 0 } else { group + 1 }, sections)
            })
            .collect();
        for index in 0..definitions.len() {
            let other = next(definitions.len());
            definitions.swap(index, other);
        }
        let notes = notes(definitions);
        let mut actual = Projection::default();
        let mut expected = Dense::default();
        for callout in &callouts {
            if next(3) == 0 {
                let collapsed = next(2) == 0;
                actual.overrides.insert(callout.source.start, collapsed);
                expected.overrides.insert(callout.source.start, collapsed);
            }
        }
        actual.rebuild(count, &callouts);
        expected.rebuild(count, &callouts);
        compare(&actual, &expected, count);
        for _ in 0..2 {
            actual.with_footnotes(count, &notes, &callouts);
            expected.with_footnotes(count, &notes, &callouts);
            compare(&actual, &expected, count);
        }
    }
}

fn compare(actual: &Projection, expected: &Dense, count: usize) {
    assert_eq!(actual.len(count), expected.len(count));
    for index in 0..expected.len(count) + 2 {
        assert_eq!(
            actual.row(index),
            expected.row(index),
            "display {index}, count {count}"
        );
    }
    for row in 0..count + 2 {
        assert_eq!(
            actual.list_index(row),
            expected.list_index(row),
            "source {row}, count {count}"
        );
    }
}

/// The previous implementation is retained only as a small-input compatibility oracle.
#[derive(Default)]
struct Dense {
    visible: Option<Vec<usize>>,
    positions: Option<Vec<usize>>,
    overrides: BTreeMap<usize, bool>,
}
impl Dense {
    fn collapsed(&self, callout: &Callout) -> bool {
        callout.collapsed.is_some()
            && self
                .overrides
                .get(&callout.source.start)
                .copied()
                .unwrap_or(callout.collapsed == Some(true))
    }
    fn rebuild(&mut self, count: usize, callouts: &[Callout]) {
        self.positions = None;
        let hidden: Vec<_> = callouts
            .iter()
            .filter(|c| self.collapsed(c))
            .map(|c| c.rows.start + 1..c.rows.end)
            .filter(|r| !r.is_empty())
            .collect();
        if hidden.is_empty() {
            self.visible = None;
            return;
        }
        let mut visible = Vec::new();
        let mut end = 0;
        for range in hidden {
            if end < range.start {
                visible.extend(end..range.start);
            }
            end = end.max(range.end);
        }
        visible.extend(end..count);
        self.visible = Some(visible);
    }
    fn with_footnotes(&mut self, count: usize, notes: &Notes, callouts: &[Callout]) {
        if notes.definitions.is_empty() {
            return;
        }
        let mut definitions = vec![false; count];
        let mut anchors = vec![usize::MAX; count];
        for callout in callouts
            .iter()
            .filter(|c| self.collapsed(c) && notes.at_row(c.rows.start).is_some())
        {
            for anchor in &mut anchors[callout.rows.start + 1..callout.rows.end] {
                if *anchor == usize::MAX {
                    *anchor = callout.rows.start;
                }
            }
        }
        for definition in &notes.definitions {
            for range in &definition.sections {
                definitions[range.clone()].fill(true);
            }
        }
        let mut visible = (0..self.len(count))
            .map(|i| self.row(i))
            .filter(|row| !definitions[*row])
            .collect::<Vec<_>>();
        for definition in notes.definitions.iter().filter(|d| d.number > 0) {
            for range in &definition.sections {
                visible.extend(range.clone().filter(|row| anchors[*row] == usize::MAX));
            }
        }
        let mut positions = vec![usize::MAX; count];
        for (index, &row) in visible.iter().enumerate() {
            positions[row] = index;
        }
        for row in 0..count {
            if positions[row] == usize::MAX {
                let anchor = if anchors[row] != usize::MAX {
                    anchors[row]
                } else {
                    self.row(self.list_index(row))
                };
                positions[row] = positions
                    .get(anchor)
                    .copied()
                    .filter(|p| *p != usize::MAX)
                    .unwrap_or(0);
            }
        }
        self.visible = Some(visible);
        self.positions = Some(positions);
    }
    fn len(&self, count: usize) -> usize {
        self.visible.as_ref().map_or(count, Vec::len)
    }
    fn row(&self, index: usize) -> usize {
        self.visible
            .as_ref()
            .map_or(index, |v| v.get(index).copied().unwrap_or(0))
    }
    fn list_index(&self, row: usize) -> usize {
        if let Some(positions) = &self.positions {
            return positions.get(row).copied().unwrap_or(0);
        }
        self.visible.as_ref().map_or(row, |v| {
            v.partition_point(|value| *value <= row).saturating_sub(1)
        })
    }
}
