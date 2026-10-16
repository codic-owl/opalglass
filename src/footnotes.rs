//! Footnote numbering is document-wide; source rows remain in source order.
//! The Reading projection moves definitions to the end without losing edit anchors.
use crate::{markdown::Span, rendering::Block};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

pub const RAW_PREFIX: &str = "opalglass-footnote-label:";
pub const TARGET_PREFIX: &str = "opalglass-footnote:";
pub const BACK_PREFIX: &str = "opalglass-footnote-back:";

pub(crate) fn raw_reference(target: &str) -> Option<(usize, &str)> {
    let (at, label) = target.strip_prefix(RAW_PREFIX)?.split_once(':')?;
    Some((at.parse().ok()?, label))
}

#[derive(Debug)]
pub struct Definition {
    pub label: Arc<str>,
    /// Zero denotes a definition that isn't referenced and should not be shown.
    pub number: usize,
    pub rows: Range<usize>,
    pub sections: Vec<Range<usize>>,
    pub references: Vec<usize>,
}
#[derive(Default, Debug)]
pub struct Notes {
    pub definitions: Vec<Definition>,
    lookup: HashMap<Arc<str>, usize>,
}
impl Notes {
    pub(crate) fn record_rows(&mut self, label: &str, rows: Range<usize>) {
        if let Some(index) = self.lookup.get(label) {
            let definition = &mut self.definitions[*index];
            if let Some(last) = definition.sections.last_mut()
                && last.end == rows.start
            {
                last.end = rows.end;
            } else {
                definition.sections.push(rows);
            }
            definition.rows = definition.sections[0].clone();
        }
    }
    pub fn at_row(&self, row: usize) -> Option<&Definition> {
        self.definitions
            .iter()
            .find(|d| d.sections.iter().any(|r| r.contains(&row)))
    }
    pub fn target_row(&self, number: usize) -> Option<usize> {
        self.definitions
            .iter()
            .find(|d| d.number == number && !d.rows.is_empty())
            .map(|d| d.rows.start)
    }
}

pub(crate) fn prepare(blocks: &mut [Block]) -> Notes {
    let labels: HashSet<_> = blocks.iter().filter_map(|b| b.footnote.clone()).collect();
    if labels.is_empty() {
        return Notes::default();
    }
    type References = Vec<(usize, Arc<str>)>;
    let mut groups: HashMap<Option<Arc<str>>, References> = HashMap::new();
    for block in blocks.iter() {
        for span in block.spans.iter().chain(
            block
                .table
                .iter()
                .flat_map(|t| t.cells.iter().flat_map(|c| c.spans.iter())),
        ) {
            if let Some((at, label)) = span.style.target.as_deref().and_then(raw_reference)
                && labels.contains(label)
            {
                groups
                    .entry(block.footnote.clone())
                    .or_default()
                    .push((at, Arc::from(label)));
            }
        }
    }
    let mut notes = Notes::default();
    let mut numbers = HashMap::new();
    register(
        &mut notes,
        &mut numbers,
        groups.remove(&None).unwrap_or_default(),
    );
    // Resolve references inside reachable definitions only, after the main text.
    // The growing queue handles chains and cycles without recursive expansion.
    let mut next = 0;
    while next < notes.definitions.len() {
        let label = notes.definitions[next].label.clone();
        register(
            &mut notes,
            &mut numbers,
            groups.remove(&Some(label)).unwrap_or_default(),
        );
        next += 1;
    }
    for label in labels {
        if !notes.lookup.contains_key(&label) {
            notes.lookup.insert(label.clone(), notes.definitions.len());
            notes.definitions.push(Definition {
                label,
                number: 0,
                rows: 0..0,
                sections: vec![],
                references: vec![],
            });
        }
    }
    for block in blocks.iter_mut() {
        if block.spans.iter().any(|s| {
            s.style
                .target
                .as_deref()
                .is_some_and(|t| t.starts_with(RAW_PREFIX))
        }) {
            (block.text, block.spans) = number_references(&block.text, &block.spans, &numbers);
        }
        if let Some(table) = &mut block.table {
            for cell in &mut table.cells {
                if !cell.spans.iter().any(|s| {
                    s.style
                        .target
                        .as_deref()
                        .is_some_and(|t| t.starts_with(RAW_PREFIX))
                }) {
                    continue;
                }
                let (text, spans) = number_references(&cell.text, &cell.spans, &numbers);
                cell.text = text.into();
                cell.spans = spans;
            }
        }
    }
    notes
}

fn register(
    notes: &mut Notes,
    numbers: &mut HashMap<usize, (usize, usize)>,
    mut references: Vec<(usize, Arc<str>)>,
) {
    references.sort_by_key(|r| r.0);
    references.dedup();
    for (at, label) in references {
        let next = notes.definitions.len();
        let index = *notes.lookup.entry(label.clone()).or_insert_with(|| {
            notes.definitions.push(Definition {
                label,
                number: next + 1,
                rows: 0..0,
                sections: vec![],
                references: vec![],
            });
            next
        });
        let definition = &mut notes.definitions[index];
        let occurrence = definition.references.len();
        numbers.insert(at, (definition.number, occurrence));
        definition.references.push(at);
    }
}

fn number_references(
    text: &str,
    spans: &[Span],
    numbers: &HashMap<usize, (usize, usize)>,
) -> (String, Vec<Span>) {
    let mut result = String::new();
    let mut styles = vec![];
    let mut end = 0;
    for span in spans {
        result.push_str(&text[end..span.range.start]);
        let start = result.len();
        let mut style = span.style.clone();
        if let Some((at, _)) = style.target.as_deref().and_then(raw_reference)
            && let Some((number, occurrence)) = numbers.get(&at)
        {
            result.push_str(&if *occurrence == 0 {
                format!("[{number}]")
            } else {
                format!("[{number}-{occurrence}]")
            });
            style.target = Some(format!("{TARGET_PREFIX}{number}"));
        } else {
            result.push_str(&text[span.range.clone()]);
        }
        styles.push(Span {
            range: start..result.len(),
            style,
        });
        end = span.range.end;
    }
    result.push_str(&text[end..]);
    (result, styles)
}

#[cfg(test)]
mod tests {
    use crate::{callouts::Projection, prepared::Prepared};

    #[test]
    fn references_number_in_use_order_and_definitions_display_at_end() {
        let source = "---\ntitle: Hidden\n---\n[^a]: First **definition**.\n\nStart[^b] then[^a] and again[^a].\n\n[^b]: Second with [link](Target.md).\n\n[^unused]: Not referenced.\n\n# End\n";
        let prepared = Prepared::new(source);
        let mut projection = Projection::default();
        projection.rebuild(prepared.rows.len(), &prepared.callouts);
        projection.with_footnotes(prepared.rows.len(), &prepared.footnotes, &prepared.callouts);
        let texts = (0..projection.len(prepared.rows.len()))
            .map(|i| prepared.rows[projection.row(i)].text.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            [
                "Start[1] then[2] and again[2-1].",
                "End",
                "Second with link.",
                "First definition."
            ]
        );
        let first = &prepared.footnotes.definitions[0];
        assert_eq!(first.label.as_ref(), "b");
        assert_eq!(first.references, [source.find("[^b] then").unwrap()]);
        let second = &prepared.footnotes.definitions[1];
        assert_eq!(
            second.references,
            [
                source.find("[^a] and").unwrap(),
                source.find("[^a].").unwrap()
            ]
        );
        for i in 0..projection.len(prepared.rows.len()) {
            assert_eq!(projection.list_index(projection.row(i)), i);
        }
        assert!(
            prepared
                .rows
                .windows(2)
                .all(|p| p[0].source.end <= p[1].source.start)
        );
        assert!(
            prepared
                .rows
                .iter()
                .flat_map(|r| r.spans.iter())
                .any(|s| s.style.bold)
        );
        let reference = prepared
            .rows
            .iter()
            .find(|r| r.text.starts_with("Start"))
            .unwrap();
        assert_eq!(
            reference.spans[0].style.target.as_deref(),
            Some("opalglass-footnote:1")
        );
    }

    #[test]
    fn folded_footnote_callouts_keep_their_own_projection_and_context() {
        let source = "Main[^n] and outer[^outer].\n\n[^n]: First\n\n    > [!tip]- Details\n    > Hidden body\n\n> [!note]- Outer\n>\n> [^outer]: Detached definition\n>\n> Ordinary hidden body\n";
        let p = Prepared::new(source);
        assert_eq!(p.callouts.len(), 2);
        let mut projection = Projection::default();
        projection.rebuild(p.rows.len(), &p.callouts);
        projection.with_footnotes(p.rows.len(), &p.footnotes, &p.callouts);
        let texts = (0..projection.len(p.rows.len()))
            .map(|i| p.rows[projection.row(i)].text.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            [
                "Main[1] and outer[2].",
                "Outer",
                "First",
                "Details",
                "Detached definition"
            ]
        );
        let inner = &p.callouts[0];
        assert_eq!(
            projection.row(projection.list_index(inner.rows.end - 1)),
            inner.rows.start
        );
        let detached = p.footnotes.target_row(2).unwrap();
        assert_eq!(p.rows[detached].quotes, 0);
        assert_eq!(p.rows[detached].indent, 0);
        projection.overrides.insert(inner.source.start, false);
        projection.rebuild(p.rows.len(), &p.callouts);
        projection.with_footnotes(p.rows.len(), &p.footnotes, &p.callouts);
        assert!(
            (0..projection.len(p.rows.len()))
                .any(|i| p.rows[projection.row(i)].text.as_ref() == "Hidden body")
        );
        for i in 0..projection.len(p.rows.len()) {
            assert_eq!(projection.list_index(projection.row(i)), i);
        }
    }

    #[test]
    fn definition_references_follow_main_text_and_cycles_are_bounded() {
        let source = "[^a]: See[^b] and self[^a].\n\nMain[^a].\n\n[^b]: Last.\n\n[^unused]: Has[^ghost].\n\n[^ghost]: Hidden.";
        let p = Prepared::new(source);
        assert_eq!(
            p.footnotes
                .definitions
                .iter()
                .filter(|d| d.number > 0)
                .count(),
            2
        );
        assert_eq!(
            p.rows[p.footnotes.target_row(1).unwrap()].text.as_ref(),
            "See[2] and self[1-1]."
        );
        assert_eq!(
            p.rows[p.footnotes.target_row(2).unwrap()].text.as_ref(),
            "Last."
        );
        assert!(p.rows.iter().any(|r| r.text.as_ref() == "Main[1]."));
    }

    #[test]
    fn code_tables_duplicate_definitions_and_unused_definitions_never_lose_other_text() {
        let source = "A[^code] and B[^table].\n\n[^code]:\n    ```rust\n    let value = 1;\n    ```\n\nOrdinary paragraph\n\n[^table]:\n    | a | b |\n    | - | - |\n    | c | d |\n\n[^code]: Duplicate\n\nAfter\n";
        let p = Prepared::new(source);
        let code = p.rows.iter().find_map(|r| r.code.as_ref()).unwrap();
        assert_eq!(code.as_ref(), "let value = 1;\n");
        assert!(p.rows.iter().any(|r| r.table.is_some()));
        let mut projection = Projection::default();
        projection.rebuild(p.rows.len(), &p.callouts);
        projection.with_footnotes(p.rows.len(), &p.footnotes, &p.callouts);
        let visible = (0..projection.len(p.rows.len()))
            .map(|i| p.rows[projection.row(i)].text.as_ref())
            .collect::<Vec<_>>();
        assert!(visible.contains(&"Ordinary paragraph"));
        assert!(visible.contains(&"After"));
        assert!(
            p.rows
                .iter()
                .all(|r| source.get(r.source.clone()).is_some())
        );
    }

    #[test]
    fn inline_footnotes_match_reference_fixture_and_preserve_formatting() {
        let source = "A reference[^n] and another[^n]. Inline ^[An inline footnote].\n\n[^n]: A **footnote** with detail.\n\nFormatted ^[**bold** and `code` and [link](Target.md)].\n\nEscaped \\^[literal] and `^[code]`.\n\nNested ^[outer [brackets] end].\n\nMultiline ^[first\nsecond].\n\nEmpty ^[].";
        let p = Prepared::new(source);
        assert_eq!(p.footnotes.definitions.len(), 6);
        let mut projection = Projection::default();
        projection.rebuild(p.rows.len(), &p.callouts);
        projection.with_footnotes(p.rows.len(), &p.footnotes, &p.callouts);
        let texts = (0..projection.len(p.rows.len()))
            .map(|i| p.rows[projection.row(i)].text.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            [
                "A reference[1] and another[1-1]. Inline [2].",
                "Formatted [3].",
                "Escaped ^[literal] and ^[code].",
                "Nested [4].",
                "Multiline [5].",
                "Empty [6].",
                "A footnote with detail.",
                "An inline footnote",
                "bold and code and link",
                "outer [brackets] end",
                "first\nsecond",
                ""
            ]
        );
        let formatted = &p.rows[p.footnotes.target_row(3).unwrap()];
        assert!(formatted.spans.iter().any(|s| s.style.bold));
        assert!(formatted.spans.iter().any(|s| s.style.code));
        assert!(
            formatted
                .spans
                .iter()
                .any(|s| s.style.target.as_deref() == Some("Target.md"))
        );
        for pair in p.rows.windows(2) {
            assert!(
                pair[0].source.end <= pair[1].source.start,
                "{:?} {:?}",
                pair[0].source,
                pair[1].source
            );
        }
        let reference_end = p.rows[0].source.end;
        assert_eq!(p.row_at_source(reference_end), 0);
    }

    #[test]
    fn unmatched_inline_markers_are_bounded_and_code_stays_literal() {
        let malformed = "^[".repeat(20_000);
        let p = Prepared::new(&malformed);
        assert!(p.footnotes.definitions.is_empty());
        let p = Prepared::new(
            "`^[literal]`\n\n```\n^[also literal]\n```\n\nReal ^[é &amp; `]` [site](Target.md)].",
        );
        assert_eq!(p.footnotes.definitions.len(), 1);
        assert_eq!(
            p.rows[p.footnotes.target_row(1).unwrap()].text.as_ref(),
            "é & ] site"
        );
    }
}
