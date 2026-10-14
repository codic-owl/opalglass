//! CPU-only document analysis. Built in the background, shared by UI frames.
use crate::{
    document,
    markdown::{self, Block, Span},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{collections::BTreeSet, ops::Range, sync::Arc};

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Heading(u8),
    Paragraph,
    Quote,
    Item,
    Task(bool),
    Code,
    Rule,
}

#[derive(Clone, Debug)]
pub struct PreviewRow {
    pub kind: Kind,
    pub text: Arc<str>,
    pub spans: Vec<Span>,
    pub continuation: bool,
    /// Shared by display chunks so copying a code block never truncates it.
    pub code: Option<Arc<str>>,
}

#[derive(Default, Debug)]
pub struct Prepared {
    pub rows: Vec<PreviewRow>,
    pub tags: Vec<String>,
    pub words: usize,
    pub links: Vec<Range<usize>>,
    pub headings: Vec<(String, usize)>,
}

impl Prepared {
    pub fn new(source: &str) -> Self {
        Self::new_cancellable(source, &AtomicBool::new(false)).unwrap()
    }
    pub fn new_cancellable(source: &str, cancelled: &AtomicBool) -> Option<Self> {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let mut result = Self {
            words: source.split_whitespace().count(),
            links: markdown::wiki_links(source)
                .into_iter()
                .map(|l| l.range)
                .collect(),
            ..Self::default()
        };
        let mut tags = BTreeSet::new();
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        for block in markdown::blocks(source) {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            let (kind, text) = match block {
                Block::Heading(level, text) => {
                    result.headings.push((text.clone(), result.rows.len()));
                    (Kind::Heading(level), text)
                }
                Block::Paragraph(text) => (Kind::Paragraph, text),
                Block::Quote(text) => (Kind::Quote, text),
                Block::Item(text) => (Kind::Item, format!("•  {text}")),
                Block::Task(done, text) => (
                    Kind::Task(done),
                    format!("{}  {text}", if done { "☑" } else { "☐" }),
                ),
                Block::Code(text) => (Kind::Code, text),
                Block::Rule => (Kind::Rule, String::new()),
            };
            let (display, spans) = if matches!(kind, Kind::Code | Kind::Rule) {
                (text, Vec::new())
            } else {
                markdown::inline(&text)
            };
            if !matches!(kind, Kind::Code | Kind::Rule) {
                let mut span_index = 0;
                for (index, _) in display.match_indices('#') {
                    while span_index < spans.len() && spans[span_index].range.end <= index {
                        span_index += 1;
                    }
                    if index > 0
                        && !display[..index]
                            .ends_with(|c: char| c.is_whitespace() || "([{ ".contains(c))
                    {
                        continue;
                    }
                    if spans.get(span_index).is_some_and(|s| {
                        s.range.contains(&index) && (s.style.code || s.style.target.is_some())
                    }) {
                        continue;
                    }
                    let tag: String = display[index + 1..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || "_-/".contains(*c))
                        .collect();
                    if !tag.is_empty() && !tag.chars().all(|c| c.is_numeric()) {
                        tags.insert(tag);
                    }
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
                result.rows.push(PreviewRow {
                    kind,
                    text: display[range].into(),
                    spans: row_spans,
                    continuation: index != 0,
                    code: code.clone(),
                });
            }
        }
        result.tags = tags.into_iter().collect();
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_chunks_share_complete_literal_copy_text() {
        let code = "let message = \"你好 **literal**\";\n".repeat(100);
        let prepared = Prepared::new(&format!("```rust\n{code}```\n"));
        assert!(prepared.rows.len() > 1);
        let first = prepared.rows[0].code.as_ref().unwrap();
        assert_eq!(first.as_ref(), code);
        for row in &prepared.rows {
            assert!(row.spans.is_empty());
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
                    && r.spans.iter().any(|s| s.style.target.is_some())
            })
            .collect();
        assert!(chunks.len() > 1);
        assert_eq!(
            chunks.iter().map(|r| r.text.as_ref()).collect::<String>(),
            label.trim()
        );
        for row in chunks {
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
