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

pub(crate) fn raw_reference(target: &str) -> Option<(Range<usize>, &str)> {
    let (start, rest) = target.strip_prefix(RAW_PREFIX)?.split_once(':')?;
    let (end, label) = rest.split_once(':')?;
    Some((start.parse().ok()?..end.parse().ok()?, label))
}

#[derive(Clone, Debug)]
pub struct Reference {
    pub source: Range<usize>,
    pub number: usize,
    pub occurrence: usize,
}
impl Reference {
    pub fn display(&self) -> String {
        if self.occurrence == 0 {
            format!("[{}]", self.number)
        } else {
            format!("[{}-{}]", self.number, self.occurrence)
        }
    }
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
    pub references: Vec<Reference>,
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
    type References = Vec<(Range<usize>, Arc<str>)>;
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
    notes.references.sort_by_key(|r| r.source.start);
    notes
}

fn register(
    notes: &mut Notes,
    numbers: &mut HashMap<usize, (usize, usize)>,
    mut references: Vec<(Range<usize>, Arc<str>)>,
) {
    references.sort_by_key(|r| r.0.start);
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
        numbers.insert(at.start, (definition.number, occurrence));
        definition.references.push(at.start);
        notes.references.push(Reference {
            source: at,
            number: definition.number,
            occurrence,
        });
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
            && let Some((number, occurrence)) = numbers.get(&at.start)
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
mod tests;
