//! Whole-document CommonMark events converted to small native display blocks.
//! Parsing once keeps reference definitions, nested containers and source offsets
//! intact; the UI only shapes the resulting visible rows.
use crate::{inline::Builder, markdown::Span, prepared::Kind};
pub use pulldown_cmark::Alignment;
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug)]
pub struct Cell {
    pub text: Arc<str>,
    pub spans: Vec<Span>,
    pub alignment: Alignment,
}
#[derive(Clone, Debug)]
pub struct TableRow {
    pub header: bool,
    pub cells: Vec<Cell>,
}
pub(crate) struct Block {
    pub footnote: Option<Arc<str>>,
    pub callout: Option<crate::callouts::Callout>,
    pub kind: Kind,
    pub source: Range<usize>,
    pub text: String,
    pub spans: Vec<Span>,
    pub indent: usize,
    pub quotes: usize,
    pub task: Option<Range<usize>>,
    pub language: Option<Arc<str>>,
    pub table: Option<TableRow>,
    pub code_mapping: Vec<(Range<usize>, Range<usize>)>,
}
impl Block {
    fn new(kind: Kind, source: Range<usize>, indent: usize, quotes: usize) -> Self {
        Self {
            footnote: None,
            callout: None,
            kind,
            source,
            text: String::new(),
            spans: vec![],
            indent,
            quotes,
            task: None,
            language: None,
            table: None,
            code_mapping: Vec::new(),
        }
    }
}
struct Pending {
    block: Block,
    inline: Builder,
    offset: usize,
}
struct Item {
    start: usize,
    marker: String,
    used: bool,
    task: Option<(bool, Range<usize>, usize)>,
}
struct Document<'a> {
    source: &'a str,
    blocks: Vec<Block>,
    pending: Option<Pending>,
    lists: Vec<Option<u64>>,
    items: Vec<Item>,
    quotes: usize,
    quote_start: Option<usize>,
    table_alignments: Vec<Alignment>,
    table: Option<Block>,
    title: Option<(crate::callouts::Callout, Range<usize>, String)>,
    footnote: Option<Arc<str>>,
    footnote_base: (usize, usize),
}
impl Document<'_> {
    fn flush(&mut self) {
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        if !matches!(pending.block.kind, Kind::Code) {
            let (text, spans, _) = pending.inline.finish(self.source.len());
            pending.block.text.push_str(&text);
            pending.block.spans = spans;
        }
        if let Some(table) = &mut self.table {
            let table = table.table.as_mut().unwrap();
            table.cells.push(Cell {
                text: pending.block.text.into(),
                spans: pending.block.spans,
                alignment: self
                    .table_alignments
                    .get(table.cells.len())
                    .copied()
                    .unwrap_or(Alignment::None),
            });
        } else if !pending.block.text.is_empty()
            || matches!(
                pending.block.kind,
                Kind::Rule | Kind::CalloutTitle | Kind::Task(_)
            )
        {
            self.blocks.push(pending.block);
        }
    }
    fn start(&mut self, mut kind: Kind, mut range: Range<usize>) {
        self.flush();
        let mut marker = None;
        let mut task = None;
        let mut indent = self.lists.len().saturating_sub(1);
        if self.table.is_none()
            && let Some(item) = self.items.last_mut()
        {
            if !item.used {
                item.used = true;
                if matches!(kind, Kind::Paragraph) {
                    kind = Kind::Item;
                    range.start = item.start;
                    marker = Some(item.marker.clone());
                    task = item.task.clone();
                }
            } else {
                indent = self.lists.len();
            }
        }
        if let Some(start) = self.quote_start.take() {
            range.start = range.start.min(start);
        }
        let mut block = Block::new(
            kind,
            range.clone(),
            indent.saturating_sub(self.footnote_base.0),
            self.quotes.saturating_sub(self.footnote_base.1),
        );
        block.footnote = self.footnote.clone();
        let mut inline = Builder::new(false);
        let mut offset = 0;
        if let Some((done, task_range, content_start)) = task {
            block.kind = Kind::Task(done);
            block.task = Some(task_range);
            offset = content_start;
        } else if let Some(marker) = marker {
            inline.literal(&marker);
        }
        self.pending = Some(Pending {
            block,
            inline,
            offset,
        });
    }
    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(Tag::Paragraph) => {
                if let Some((callout, title, default_title)) = self.title.take() {
                    self.start(Kind::CalloutTitle, callout.source.start..title.end);
                    let pending = self.pending.as_mut().unwrap();
                    pending.offset = title.start;
                    if title.is_empty() {
                        pending.inline.literal(&default_title);
                    }
                    pending.block.callout = Some(callout);
                } else {
                    self.start(Kind::Paragraph, range);
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                self.start(Kind::Heading(level as u8), range)
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.flush();
                self.quotes += 1;
                self.quote_start.get_or_insert(range.start);
                self.title = crate::callouts::Callout::parse(self.source, range);
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush();
                self.quotes = self.quotes.saturating_sub(1);
                self.quote_start = None;
                self.title = None;
            }
            Event::Start(Tag::List(number)) => {
                self.flush();
                self.lists.push(number);
            }
            Event::End(TagEnd::List(_)) => {
                self.flush();
                self.lists.pop();
            }
            Event::Start(Tag::Item) => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}.  ");
                        *number += 1;
                        marker
                    }
                    _ => "•  ".into(),
                };
                self.items.push(Item {
                    start: range.start,
                    marker,
                    used: false,
                    task: crate::editing::list_prefix(
                        self.source[range.clone()]
                            .lines()
                            .next()
                            .unwrap_or_default(),
                    )
                    .and_then(|p| Some((p.task?, p.task_range?, p.end)))
                    .map(|(done, task, end)| {
                        (
                            done,
                            range.start + task.start..range.start + task.end,
                            range.start + end,
                        )
                    }),
                });
            }
            Event::End(TagEnd::Item) => {
                self.flush();
                self.items.pop();
            }
            Event::Start(Tag::CodeBlock(code)) => {
                self.start(Kind::Code, range);
                if let CodeBlockKind::Fenced(info) = code {
                    self.pending.as_mut().unwrap().block.language =
                        info.split_whitespace().next().map(Arc::from);
                }
            }
            Event::End(TagEnd::CodeBlock | TagEnd::Paragraph | TagEnd::Heading(_)) => self.flush(),
            Event::Rule => {
                self.start(Kind::Rule, range);
                self.flush();
            }
            Event::Start(Tag::Table(alignments)) => {
                self.flush();
                self.table_alignments = alignments;
            }
            Event::End(TagEnd::Table) => {
                self.flush();
                self.table_alignments.clear();
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                self.start(Kind::Table, range);
                let header = matches!(event, Event::Start(Tag::TableHead));
                let mut block = self.pending.take().unwrap().block;
                block.table = Some(TableRow {
                    header,
                    cells: vec![],
                });
                self.table = Some(block);
            }
            Event::Start(Tag::TableCell) => self.start(Kind::Paragraph, range),
            Event::End(TagEnd::TableCell) => self.flush(),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                self.flush();
                if let Some(table) = self.table.take() {
                    self.blocks.push(table);
                }
            }
            Event::TaskListMarker(done) => {
                if self.pending.is_none() {
                    self.start(Kind::Paragraph, range.clone());
                }
                let pending = self.pending.as_mut().unwrap();
                if !self.source[range.end..].starts_with([' ', '\t']) {
                    pending.inline.literal(&self.source[range]);
                    return;
                }
                pending.block.kind = Kind::Task(done);
                pending.block.task = Some(range.clone());
                pending.inline = Builder::new(false);
                // The clickable checkbox is a separate native element.
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                self.flush();
                self.footnote = Some(label.into_string().into());
                self.footnote_base = (self.lists.len(), self.quotes);
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                self.flush();
                self.footnote = None;
                self.footnote_base = (0, 0);
            }
            event => {
                if matches!(event, Event::SoftBreak | Event::HardBreak)
                    && self
                        .pending
                        .as_ref()
                        .is_some_and(|p| matches!(p.block.kind, Kind::CalloutTitle))
                {
                    self.flush();
                    return;
                }
                if self.pending.as_ref().is_some_and(|p| {
                    matches!(p.block.kind, Kind::CalloutTitle) && range.start >= p.block.source.end
                }) {
                    self.flush();
                    if matches!(event, Event::SoftBreak | Event::HardBreak) {
                        return;
                    }
                }
                if self.pending.is_none() {
                    self.start(Kind::Paragraph, range.clone());
                }
                let pending = self.pending.as_mut().unwrap();
                pending.block.source.end = pending.block.source.end.max(range.end);
                if matches!(pending.block.kind, Kind::Code) {
                    if let Event::Text(text) = event {
                        if self.source.get(range.clone()) == Some(text.as_ref()) {
                            pending.block.code_mapping.push((
                                pending.block.text.len()..pending.block.text.len() + text.len(),
                                range,
                            ));
                        }
                        pending.block.text.push_str(&text);
                    }
                } else {
                    pending
                        .inline
                        .event(event, range, self.source, pending.offset);
                }
            }
        }
    }
}
pub(crate) fn parse(source: &str, cancelled: &AtomicBool) -> Option<Vec<Block>> {
    let visible = crate::links::visible_source(source);
    let source = &visible.text;
    let mut inline_notes = crate::inline_notes::discover(source);
    let mut document = Document {
        source,
        blocks: vec![],
        pending: None,
        lists: vec![],
        items: vec![],
        quotes: 0,
        quote_start: None,
        table_alignments: vec![],
        table: None,
        title: None,
        footnote: None,
        footnote_base: (0, 0),
    };
    for (event, range) in Parser::new_ext(source, crate::links::options()).into_offset_iter() {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        if inline_notes.is_empty() {
            document.event(event, range);
        } else {
            crate::inline_notes::route(&mut inline_notes, event, range, source, |event, range| {
                document.event(event, range)
            });
        }
    }
    document.flush();
    if !inline_notes.is_empty() {
        let mut notes = inline_notes.into_iter().filter(|n| n.emitted).peekable();
        let mut blocks = Vec::with_capacity(document.blocks.len());
        for block in document.blocks {
            let end = block.source.end;
            blocks.push(block);
            while notes.peek().is_some_and(|n| n.range.start < end) {
                let note = notes.next().unwrap();
                let mut block = Block::new(Kind::Paragraph, end..end, 0, 0);
                block.footnote = Some(note.label.into());
                (block.text, block.spans, _) = note.inline.finish(source.len());
                blocks.push(block);
            }
        }
        document.blocks = blocks;
    }
    for block in &mut document.blocks {
        for (_, range) in &mut block.code_mapping {
            *range = visible.original_offset(range.start)..visible.original_offset(range.end);
        }
        for span in block.spans.iter_mut().chain(
            block
                .table
                .iter_mut()
                .flat_map(|t| t.cells.iter_mut().flat_map(|c| c.spans.iter_mut())),
        ) {
            if let Some((at, label)) = span
                .style
                .target
                .as_deref()
                .and_then(crate::footnotes::raw_reference)
            {
                span.style.target = Some(format!(
                    "{}{at}:{label}",
                    crate::footnotes::RAW_PREFIX,
                    at = visible.original_offset(at)
                ));
            }
        }
        if let Some(callout) = &mut block.callout {
            callout.source = visible.original_offset(callout.source.start)
                ..visible.original_offset(callout.source.end);
        }
        block.source =
            visible.original_offset(block.source.start)..visible.original_offset(block.source.end);
        if let Some(range) = &mut block.task {
            *range = visible.original_offset(range.start)..visible.original_offset(range.end);
        }
    }
    Some(document.blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quote_and_list_anchors_are_consumed_by_rules_and_tables() {
        let source = "> ***\n>\n> Paragraph\n\n> | a | b |\n> | - | - |\n> | c | d |\n>\n> After table\n\n- ***\n\n  After rule\n";
        let blocks = parse(source, &AtomicBool::new(false)).unwrap();
        for pair in blocks.windows(2) {
            assert!(
                pair[0].source.end <= pair[1].source.start,
                "{:?} {:?}",
                pair[0].source,
                pair[1].source
            );
        }
        assert_eq!(blocks[0].source.start, 0);
        assert!(matches!(blocks[0].kind, Kind::Rule));
        assert_eq!(blocks[1].text, "Paragraph");
        assert_eq!(blocks[2].source.start, source.find("> | a").unwrap());
        assert_eq!(blocks.last().unwrap().text, "After rule");
    }

    #[test]
    fn document_context_preserves_references_nested_lists_quotes_and_tables() {
        let source = "Title\n=====\n\n3. First **bold**\n   - Child\n     > Nested quote\n4. Second\n\n| Left | Right |\n| :--- | ---: |\n| [Alias][dest] | `x\\|y` |\n| [[Target\\|alias]] | end |\n\n[dest]: <Folder/Target.md> \"title\"\n\n    indented **literal**\n";
        let blocks = parse(source, &AtomicBool::new(false)).unwrap();
        assert!(matches!(blocks[0].kind, Kind::Heading(1)));
        assert_eq!(blocks[0].text, "Title");
        assert_eq!(blocks[1].text, "3.  First bold");
        assert_eq!(blocks[2].text, "•  Child");
        assert_eq!(blocks[2].indent, 1);
        assert_eq!(blocks[3].text, "Nested quote");
        assert_eq!(blocks[3].quotes, 1);
        assert_eq!(blocks[4].text, "4.  Second");
        let tables: Vec<_> = blocks.iter().filter_map(|b| b.table.as_ref()).collect();
        assert_eq!(tables.len(), 3);
        assert_eq!(
            tables[2].cells[0].spans[0].style.target.as_deref(),
            Some("Target")
        );
        assert!(tables[0].header);
        assert_eq!(tables[1].cells[0].text.as_ref(), "Alias");
        assert_eq!(
            tables[1].cells[0].spans[0].style.target.as_deref(),
            Some("Folder/Target.md")
        );
        assert_eq!(tables[1].cells[1].text.as_ref(), "x|y");
        assert_eq!(tables[1].cells[1].alignment, Alignment::Right);
        assert!(tables[1].cells[1].spans[0].style.code);
        assert_eq!(blocks.last().unwrap().text, "indented **literal**\n");
        assert!(matches!(blocks.last().unwrap().kind, Kind::Code));
        assert!(
            blocks
                .iter()
                .all(|b| source.get(b.source.clone()).is_some())
        );
    }

    #[test]
    fn tasks_have_exact_edit_ranges_and_code_stays_literal() {
        let source = "- [ ] **Todo**\n  - [X] Done\n\n```rust\n- [ ] literal\n```\n\nSoft\nbreak\n";
        let blocks = parse(source, &AtomicBool::new(false)).unwrap();
        assert!(matches!(blocks[0].kind, Kind::Task(false)));
        assert_eq!(blocks[0].text, "Todo");
        assert_eq!(&source[blocks[0].task.clone().unwrap()], "[ ]");
        assert!(matches!(blocks[1].kind, Kind::Task(true)));
        assert_eq!(blocks[1].indent, 1);
        assert_eq!(&source[blocks[1].task.clone().unwrap()], "[X]");
        assert_eq!(blocks[2].text, "- [ ] literal\n");
        assert_eq!(blocks[2].language.as_deref(), Some("rust"));
        assert_eq!(blocks[3].text, "Soft\nbreak");
    }
    #[test]
    fn custom_tasks_support_all_list_markers_and_unicode_without_eating_text() {
        let source = "- [?] Question\n  * [/] **Nested**\n+ [-] Cancelled\n\n1. [é] Unicode\n2. [ ]\n\n- [?]no-space\n- [word] Not a task\n\n`- [?] literal`";
        let blocks = parse(source, &AtomicBool::new(false)).unwrap();
        let tasks: Vec<_> = blocks.iter().filter(|b| b.task.is_some()).collect();
        assert_eq!(tasks.len(), 4);
        assert_eq!(
            tasks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            ["Question", "Nested", "Cancelled", "Unicode"]
        );
        for (task, marker) in tasks.iter().zip(["[?]", "[/]", "[-]", "[é]"]) {
            assert_eq!(&source[task.task.clone().unwrap()], marker);
        }
        assert_eq!(tasks[1].indent, 1);
        assert!(tasks[1].spans[0].style.bold);
        assert!(
            blocks
                .iter()
                .any(|b| b.text.contains("[?]no-space") && b.task.is_none())
        );
    }
    #[test]
    fn hidden_comments_do_not_create_code_or_shift_task_edit_positions() {
        let source = "---\ntitle: Hidden\n---\n%% comment %%Visible **text**\n\n%%\n# Hidden\n%%\n\n- [ ] Real task\n\n<!-- hidden -->\n\n`%% literal %%`";
        let blocks = parse(source, &AtomicBool::new(false)).unwrap();
        assert_eq!(blocks.len(), 3);
        assert!(matches!(blocks[0].kind, Kind::Paragraph));
        assert_eq!(blocks[0].text, "Visible text");
        assert_eq!(&source[blocks[1].task.clone().unwrap()], "[ ]");
        assert_eq!(blocks[2].text, "%% literal %%");
        assert!(blocks[2].spans[0].style.code);
    }
}
