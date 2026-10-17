//! Turn parsed blocks into bounded display rows before they reach the UI.
use super::*;
use crate::{
    document,
    rendering::{Block, TableRow},
};

impl Prepared {
    pub(super) fn append_block(
        &mut self,
        mut block: Block,
        source: &str,
        tags: &mut BTreeSet<String>,
    ) {
        let kind = if block.quotes > 0 && matches!(block.kind, Kind::Paragraph) {
            Kind::Quote
        } else {
            block.kind
        };
        if let Some(mut callout) = block.callout.take() {
            callout.rows.start = self.rows.len();
            self.callouts.push(callout);
        }
        let first_row = self.rows.len();
        if matches!(kind, Kind::Heading(_)) {
            self.headings.push((block.text.clone(), first_row));
        }
        for span in block.spans.iter().chain(
            block
                .table
                .iter()
                .flat_map(|table| table.cells.iter().flat_map(|cell| cell.spans.iter())),
        ) {
            if let Some(tag) = span
                .style
                .target
                .as_deref()
                .and_then(|t| t.strip_prefix(markdown::TAG_TARGET_PREFIX))
            {
                tags.insert(tag.to_owned());
            }
        }
        let footnote = block.footnote.clone();
        if let Some(table) = block.table.take() {
            self.append_table(&block, table, kind, source);
        } else {
            self.append_text(block, kind, source);
        }
        if let Some(label) = footnote {
            self.footnotes
                .record_rows(&label, first_row..self.rows.len());
        }
    }

    fn append_table(&mut self, block: &Block, table: TableRow, kind: Kind, source: &str) {
        let chunks = crate::tables::chunks(table);
        let count = chunks.len();
        for (index, chunk) in chunks.into_iter().enumerate() {
            self.rows.push(PreviewRow {
                kind,
                indent: block.indent,
                quotes: block.quotes,
                task: None,
                language: None,
                table: Some(chunk),
                source: boundary(
                    source,
                    block.source.start + block.source.len() * index / count,
                )
                    ..boundary(
                        source,
                        block.source.start + block.source.len() * (index + 1) / count,
                    ),
                text: Arc::from(""),
                spans: Vec::new(),
                continuation: index != 0,
                code: None,
            });
        }
    }

    fn append_text(&mut self, block: Block, kind: Kind, source: &str) {
        let display = block.text;
        let spans = block.spans;
        let source_range = block.source;
        let mut span_index = 0;
        let code: Option<Arc<str>> = matches!(kind, Kind::Code).then(|| display.clone().into());
        for (index, range) in document::chunks(&display, 1024).into_iter().enumerate() {
            while span_index < spans.len() && spans[span_index].range.end <= range.start {
                span_index += 1;
            }
            let row_spans = spans[span_index..]
                .iter()
                .take_while(|s| s.range.start < range.end)
                .map(|s| Span {
                    range: s.range.start.max(range.start) - range.start
                        ..s.range.end.min(range.end) - range.start,
                    style: s.style.clone(),
                })
                .collect();
            self.rows.push(PreviewRow {
                kind,
                indent: block.indent,
                quotes: block.quotes,
                task: block.task.clone(),
                language: block.language.clone(),
                table: None,
                // Interpolate chunks inside a block; boundaries between blocks
                // remain exact despite removed markers and different wrapping.
                source: boundary(
                    source,
                    source_range.start + source_range.len() * range.start / display.len().max(1),
                )..if range.end == display.len() {
                    source_range.end
                } else {
                    boundary(
                        source,
                        source_range.start + source_range.len() * range.end / display.len().max(1),
                    )
                },
                text: display[range].into(),
                spans: row_spans,
                continuation: index != 0,
                code: code.clone(),
            });
        }
    }
}

fn boundary(source: &str, mut index: usize) -> usize {
    while !source.is_char_boundary(index) {
        index -= 1;
    }
    index
}
