//! CPU-only document analysis. Built in the background, shared by UI frames.
use crate::{
    document,
    markdown::{self, Span, WikiLink},
};
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
    pub table: Option<Arc<crate::rendering::TableRow>>,
    pub source: std::ops::Range<usize>,
    pub text: Arc<str>,
    pub spans: Vec<Span>,
    pub continuation: bool,
    /// Shared by display chunks so copying a code block never truncates it.
    pub code: Option<Arc<str>>,
}

#[derive(Default, Debug)]
pub struct Prepared {
    pub syntax_ready: bool,
    pub source_syntax: Arc<Vec<crate::syntax::Span>>,
    pub footnotes: crate::footnotes::Notes,
    pub callouts: Vec<crate::callouts::Callout>,
    /// False only for the bounded preview shown on the first frame of opening.
    pub complete: bool,
    pub rows: Vec<PreviewRow>,
    pub tags: Vec<String>,
    pub words: usize,
    pub links: Vec<WikiLink>,
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
        let mut result = Self {
            complete: true,
            syntax_ready: true,
            frontmatter: crate::properties::frontmatter(source)
                .ok()
                .flatten()
                .map(|h| h.range),
            words: source.split_whitespace().count(),
            links: markdown::wiki_links(source),
            ..Self::default()
        };
        let mut tags = BTreeSet::new();
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let mut blocks = crate::rendering::parse(source, cancelled)?;
        result.footnotes = crate::footnotes::prepare(&mut blocks);
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
            let kind = if block.quotes > 0 && matches!(block.kind, Kind::Paragraph) {
                Kind::Quote
            } else {
                block.kind
            };
            if let Some(mut callout) = block.callout {
                callout.rows.start = result.rows.len();
                result.callouts.push(callout);
            }
            let source_range = block.source;
            let first_row = result.rows.len();
            let display = block.text;
            let spans = block.spans;
            if matches!(kind, Kind::Heading(_)) {
                result.headings.push((display.clone(), result.rows.len()));
            }
            let table = block.table.map(Arc::new);
            for span in spans.iter().chain(
                table
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
            // Also bound individual paragraphs and fenced code blocks: a single
            // enormous block must not force the text system to shape megabytes.
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
                let boundary = |mut index: usize| {
                    while !source.is_char_boundary(index) {
                        index -= 1;
                    }
                    index
                };
                result.rows.push(PreviewRow {
                    kind,
                    indent: block.indent,
                    quotes: block.quotes,
                    task: block.task.clone(),
                    language: block.language.clone(),
                    table: table.clone(),
                    // Interpolate chunks inside a block; boundaries between blocks
                    // remain exact despite removed markers and different wrapping.
                    source: boundary(
                        source_range.start
                            + source_range.len() * range.start / display.len().max(1),
                    )..if range.end == display.len() {
                        source_range.end
                    } else {
                        boundary(
                            source_range.start
                                + source_range.len() * range.end / display.len().max(1),
                        )
                    },
                    text: display[range].into(),
                    spans: row_spans,
                    continuation: index != 0,
                    code: code.clone(),
                });
            }
            if let Some(label) = &block.footnote {
                result
                    .footnotes
                    .record_rows(label, first_row..result.rows.len());
            }
        }
        tags.extend(crate::properties::tags(source));
        for callout in &mut result.callouts {
            callout.rows.end = result
                .rows
                .partition_point(|r| r.source.start < callout.source.end);
        }
        result.tags = tags.into_iter().collect();
        result.source_syntax = Arc::new(source_syntax);
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_colors_keep_exact_source_ranges_inside_containers_and_hidden_metadata() {
        let source = "---\ntitle: Example\n---\n%%hidden%%\n> ```js\n> const café = \"你好\";\n> /* comment\n> continuation */\n> ```\n\n- Item\n\n  ```python\n  print(\"hello\")\n  ```\n";
        let opening = Prepared::opening(source);
        assert!(opening.complete);
        assert!(!opening.analysis_complete());
        assert!(opening.source_syntax.is_empty());
        assert!(
            opening
                .rows
                .iter()
                .all(|r| r.spans.iter().all(|s| s.style.syntax_color.is_none()))
        );
        let p = Prepared::new(source);
        assert!(p.analysis_complete());
        assert!(!p.source_syntax.is_empty());
        assert!(
            p.source_syntax
                .windows(2)
                .all(|s| s[0].range.end <= s[1].range.start)
        );
        assert!(
            p.source_syntax
                .iter()
                .all(|s| source.get(s.range.clone()).is_some())
        );
        let painted = p
            .source_syntax
            .iter()
            .map(|s| &source[s.range.clone()])
            .collect::<String>();
        assert_eq!(
            painted,
            "const café = \"你好\";\n/* comment\ncontinuation */\nprint(\"hello\")\n"
        );
        assert_eq!(
            p.rows
                .iter()
                .filter_map(|r| r.code.as_ref())
                .next()
                .unwrap()
                .as_ref(),
            "const café = \"你好\";\n/* comment\ncontinuation */\n"
        );
        assert!(Prepared::opening("# Plain\n\nNo code").analysis_complete());
    }

    #[test]
    fn code_chunks_keep_multiline_color_and_complete_copy_text() {
        let code = format!("/*\n{}*/\nconst x = 42;\n", "é comment\n".repeat(400));
        let source = format!("```javascript\n{code}```\n");
        let p = Prepared::new(&source);
        let rows = p
            .rows
            .iter()
            .filter(|r| matches!(r.kind, Kind::Code))
            .collect::<Vec<_>>();
        assert!(rows.len() > 2);
        assert_eq!(
            rows.iter().map(|r| r.text.as_ref()).collect::<String>(),
            code
        );
        let color = rows[0].spans[0].style.syntax_color;
        assert_eq!(rows[1].spans[0].style.syntax_color, color);
        assert_eq!(rows[1].code.as_deref(), Some(code.as_str()));
        for row in rows {
            for span in &row.spans {
                assert!(row.text.get(span.range.clone()).is_some());
            }
        }
    }

    #[test]
    fn preview_source_ranges_track_blocks_and_long_chunks() {
        let source = format!(
            "\r\n# Héading\r\n\r\nA **bold** paragraph\r\ncontinued.\r\n\r\n> quote\r\n- [x] task\r\n\r\n```rust\r\n{}\r\n```\r\n\r\n---\r\n\r\n# End",
            "let x = 你好;\r\n".repeat(500)
        );
        let prepared = Prepared::new(&source);
        assert_eq!(
            prepared.rows[0].source.start,
            source.find("# Héading").unwrap()
        );
        assert_eq!(
            prepared.rows[1].source.start,
            source.find("A **bold**").unwrap()
        );
        assert_eq!(
            prepared.rows[2].source.start,
            source.find("> quote").unwrap()
        );
        assert_eq!(prepared.rows[3].source.start, source.find("- [x]").unwrap());
        let code: Vec<_> = prepared
            .rows
            .iter()
            .filter(|r| matches!(r.kind, Kind::Code))
            .collect();
        assert!(code.len() > 2);
        assert_eq!(code[0].source.start, source.find("```rust").unwrap());
        for pair in code.windows(2) {
            assert_eq!(pair[0].source.end, pair[1].source.start);
        }
        assert_eq!(
            prepared.rows.last().unwrap().source,
            source.find("# End").unwrap()..source.len()
        );
        assert!(
            prepared
                .rows
                .windows(2)
                .all(|r| r[0].source.end <= r[1].source.start)
        );
        let unfinished = Prepared::new("# Before\n\n```\nhello");
        assert_eq!(unfinished.rows.last().unwrap().source, 10..19);
    }

    #[test]
    fn opening_preview_is_bounded_and_short_notes_are_complete() {
        let short = "# Hello\n\nText with [[Note]] and #tag.";
        let small = Prepared::opening(short);
        assert!(small.complete);
        assert_eq!(small.tags, vec!["tag"]);
        assert_eq!(small.links.len(), 1);

        let large = format!("{short}\n\n{}", "More text 👩‍💻 e\u{301}.\n\n".repeat(10_000));
        let initial = Prepared::opening(&large);
        let full = Prepared::new(&large);
        assert!(!initial.complete);
        assert!(full.complete);
        assert!(initial.rows.len() < full.rows.len() / 10);
        for (early, complete) in initial.rows.iter().zip(&full.rows).take(10) {
            assert_eq!(early.text, complete.text);
        }
        // A huge first line must also be bounded and tolerate a multibyte cut.
        let initial = Prepared::opening(&"界".repeat(10_000));
        assert!(!initial.complete);
        assert!(initial.rows.iter().map(|r| r.text.len()).sum::<usize>() <= 8192);
        let initial = Prepared::opening(&format!("```\n{}\n```", "code\n".repeat(10_000)));
        assert!(!initial.complete);
        assert!(initial.rows[0].code.as_ref().unwrap().len() < 8192);
    }

    #[test]
    fn code_chunks_share_complete_literal_copy_text() {
        let code = "let message = \"你好 **literal**\";\n".repeat(100);
        let prepared = Prepared::new(&format!("```rust\n{code}```\n"));
        assert!(prepared.rows.len() > 1);
        let first = prepared.rows[0].code.as_ref().unwrap();
        assert_eq!(first.as_ref(), code);
        for row in &prepared.rows {
            assert!(row.spans.iter().all(|s| s.style.target.is_none()
                && !s.style.image
                && s.style.highlight.is_none()));
            assert!(Arc::ptr_eq(first, row.code.as_ref().unwrap()));
        }
    }
    #[test]
    fn chunks_preserve_unicode_spans_targets_and_heading_locations() {
        let label = "ş😀e\u{301} ".repeat(700);
        let source = format!(
            "# First\n\n**[[Target|{label}]]**\n\n# Last\n\n#tag `#ignored`\n```\n#hidden\n```"
        );
        let prepared = Prepared::new(&source);
        let chunks: Vec<_> = prepared
            .rows
            .iter()
            .filter(|r| {
                matches!(r.kind, Kind::Paragraph)
                    && r.spans
                        .iter()
                        .any(|s| s.style.target.as_deref() == Some("Target"))
            })
            .collect();
        assert!(chunks.len() > 1);
        assert_eq!(
            chunks.iter().map(|r| r.text.as_ref()).collect::<String>(),
            label.trim()
        );
        for row in chunks {
            assert!(source.get(row.source.clone()).is_some());
            for span in &row.spans {
                assert!(row.text.get(span.range.clone()).is_some());
                assert!(span.style.bold);
                assert_eq!(span.style.target.as_deref(), Some("Target"));
            }
        }
        assert_eq!(prepared.tags, markdown::tags(&source));
        let (_, last) = prepared.headings.last().unwrap();
        assert!(matches!(prepared.rows[*last].kind, Kind::Heading(1)));
        assert_eq!(prepared.rows[*last].text.as_ref(), "Last");
    }
}
