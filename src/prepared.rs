//! CPU-only document analysis. Built in the background, shared by UI frames.
use crate::markdown::{self, Span, WikiLink};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    CalloutTitle,
    Heading(u8),
    Paragraph,
    Quote,
    Item,
    Task(bool),
    Code,
    Rule,
    Table,
}

#[derive(Clone, Debug)]
pub struct PreviewRow {
    pub kind: Kind,
    pub indent: usize,
    pub quotes: usize,
    pub task: Option<std::ops::Range<usize>>,
    pub language: Option<Arc<str>>,
    pub table: Option<Arc<crate::tables::Chunk>>,
    pub source: std::ops::Range<usize>,
    pub text: Arc<str>,
    pub spans: Vec<Span>,
    pub continuation: bool,
    /// Shared by display chunks so copying a code block never truncates it.
    pub code: Option<Arc<str>>,
}

#[derive(Default, Debug)]
pub struct Prepared {
    pub embeds: Vec<crate::embeds::Occurrence>,
    pub live: Arc<crate::live::Context>,
    pub anchors: crate::anchors::Index,
    pub syntax_ready: bool,
    pub source_syntax: Arc<Vec<crate::syntax::Span>>,
    pub footnotes: crate::footnotes::Notes,
    pub callouts: Vec<crate::callouts::Callout>,
    /// False only for the bounded preview shown on the first frame of opening.
    pub complete: bool,
    pub rows: Vec<PreviewRow>,
    pub tags: Vec<String>,
    pub words: usize,
    pub links: Arc<Vec<WikiLink>>,
    pub headings: Vec<(String, usize)>,
    pub frontmatter: Option<std::ops::Range<usize>>,
}

impl Prepared {
    pub fn analysis_complete(&self) -> bool {
        self.complete && self.syntax_ready
    }
    pub fn row_at_source(&self, byte: usize) -> usize {
        let mut row = self
            .rows
            .partition_point(|r| r.source.start <= byte)
            .saturating_sub(1);
        // Inline footnote definitions have a display-only, zero-length anchor
        // after their paragraph. Scrolling a blank source line must not jump to
        // the footer merely because these synthetic rows share that boundary.
        while row > 0 && self.rows[row].source.is_empty() {
            row -= 1;
        }
        row
    }
    pub fn opening(source: &str) -> Self {
        const INITIAL_BYTES: usize = 8 * 1024;
        if source.len() <= INITIAL_BYTES {
            return Self::build(source, &AtomicBool::new(false), false).unwrap();
        }
        let mut end = INITIAL_BYTES;
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        // Prefer a complete source line, without letting a very long first line
        // force the entire document through the parser before its first frame.
        if let Some(newline) = source[..end].rfind('\n') {
            end = newline + 1;
        }
        let mut result = Self::build(&source[..end], &AtomicBool::new(false), false).unwrap();
        result.complete = false;
        result
    }
    pub fn new(source: &str) -> Self {
        Self::new_cancellable(source, &AtomicBool::new(false)).unwrap()
    }
    pub fn new_cancellable(source: &str, cancelled: &AtomicBool) -> Option<Self> {
        Self::build(source, cancelled, true)
    }
    pub fn undecorated_cancellable(source: &str, cancelled: &AtomicBool) -> Option<Self> {
        Self::build(source, cancelled, false)
    }
    fn build(source: &str, cancelled: &AtomicBool, decorate: bool) -> Option<Self> {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let analysis = crate::links::analyze(source);
        let mut result = Self {
            anchors: analysis.anchors,
            complete: true,
            syntax_ready: true,
            frontmatter: crate::properties::frontmatter(source)
                .ok()
                .flatten()
                .map(|h| h.range),
            words: source.split_whitespace().count(),
            links: Arc::new(analysis.inline_links),
            ..Self::default()
        };
        let mut tags = BTreeSet::new();
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let mut blocks = crate::rendering::parse_with_anchors(source, cancelled, &result.anchors)?;
        result.footnotes = crate::footnotes::prepare(&mut blocks);
        result.live = Arc::new(
            crate::live::Context::new(
                source,
                analysis.hidden,
                analysis.definitions,
                result.links.clone(),
                &result.footnotes,
                &result.anchors,
                &result.frontmatter,
            )
            .with_definitions(source, &blocks, &result.footnotes)
            .with_callouts(source, &blocks),
        );
        let mut syntax_budget = crate::syntax::DOCUMENT_BYTES;
        let mut source_syntax = Vec::new();
        for mut block in blocks {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            if matches!(block.kind, Kind::Code)
                && let Some(language) = &block.language
            {
                if decorate {
                    let syntax = crate::syntax::highlight(
                        &block.text,
                        language,
                        cancelled,
                        &mut syntax_budget,
                    )?;
                    for (display, original) in &block.code_mapping {
                        if source.get(original.clone()) != block.text.get(display.clone()) {
                            continue;
                        }
                        let first = syntax.partition_point(|s| s.range.end <= display.start);
                        for span in syntax[first..]
                            .iter()
                            .take_while(|s| s.range.start < display.end)
                        {
                            source_syntax.push(crate::syntax::Span {
                                range: original.start + span.range.start.max(display.start)
                                    - display.start
                                    ..original.start + span.range.end.min(display.end)
                                        - display.start,
                                style: span.style,
                            });
                        }
                    }
                    block.spans = syntax
                        .into_iter()
                        .map(|span| Span {
                            range: span.range,
                            style: markdown::InlineStyle {
                                syntax_color: Some(span.style.color),
                                bold: span.style.bold,
                                italic: span.style.italic,
                                ..Default::default()
                            },
                        })
                        .collect();
                } else {
                    result.syntax_ready = false;
                }
            }
            result.append_block(block, source, &mut tags);
        }
        tags.extend(crate::properties::tags(source));
        for callout in &mut result.callouts {
            callout.rows.end = result
                .rows
                .partition_point(|r| r.source.start < callout.source.end);
        }
        result.tags = tags.into_iter().collect();
        result.source_syntax = Arc::new(source_syntax);
        result.embeds = crate::embeds::occurrences(&result.rows);
        Some(result)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod block_markers_tests;

mod rows;
