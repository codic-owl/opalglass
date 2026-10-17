//! A display-only Markdown projection. Editing and history retain the source.
use crate::markdown::{self, Span};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    CalloutTitle,
    Heading(u8),
    Quote,
    Code,
    Fence,
    Rule,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub text: String,
    pub spans: Vec<Span>,
    pub kind: Kind,
    /// Empty for identity projections; otherwise one entry per display byte.
    mapping: Vec<usize>,
    source_len: usize,
}

impl Line {
    pub fn raw(source: &str) -> Self {
        Self {
            text: source.into(),
            spans: Vec::new(),
            kind: Kind::Text,
            mapping: Vec::new(),
            source_len: source.len(),
        }
    }
    pub fn new(source: &str, reveal: bool, code: bool, continuation: bool) -> Self {
        Self::parsed(source, reveal, code, continuation, |text, _| {
            markdown::inline_mapped(text)
        })
    }
    fn parsed(
        source: &str,
        reveal: bool,
        code: bool,
        continuation: bool,
        parse: impl FnOnce(&str, usize) -> (String, Vec<Span>, Vec<usize>),
    ) -> Self {
        let mut line = Self::raw(source);
        let trimmed = source.trim_start();
        let indent = source.len() - trimmed.len();
        let mut skip = 0;
        let mut prefix = "";
        let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if code || fence {
            line.kind = if fence { Kind::Fence } else { Kind::Code };
            if fence && !reveal {
                let marker = trimmed.as_bytes()[0];
                skip = indent + trimmed.bytes().take_while(|&c| c == marker).count();
                line.text = source[skip..].trim_start().into();
                skip = source.len() - source[skip..].trim_start().len();
                line.mapping = (skip..=source.len()).collect();
            }
            return line;
        }
        if !continuation {
            let hashes = trimmed.bytes().take_while(|&c| c == b'#').count();
            if (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ') {
                line.kind = Kind::Heading(hashes as u8);
                skip = indent + hashes + 1;
            } else if trimmed.starts_with("> ") {
                line.kind = Kind::Quote;
                skip = indent + 2;
            } else if ["---", "***", "___"].contains(&trimmed.trim_end()) {
                line.kind = Kind::Rule;
                if !reveal {
                    line.text.clear();
                    line.mapping = vec![source.len()];
                }
                return line;
            } else if let Some(list) = crate::editing::list_prefix(source)
                && let Some(done) = list.task
            {
                skip = list.end;
                prefix = if done { "☑  " } else { "☐  " };
            } else if ["- ", "* ", "+ "].iter().any(|p| trimmed.starts_with(p)) {
                skip = indent + 2;
                prefix = "•  ";
            }
        }
        if reveal {
            return line;
        }
        let (text, spans, mapping) = parse(&source[skip..], skip);
        line.text = format!("{prefix}{text}");
        line.spans = spans
            .into_iter()
            .map(|mut span| {
                span.range = span.range.start + prefix.len()..span.range.end + prefix.len();
                span
            })
            .collect();
        line.mapping = vec![0; prefix.len()];
        line.mapping.extend(mapping.into_iter().map(|i| i + skip));
        if line.mapping.len() == line.text.len() {
            line.mapping.push(source.len());
        }
        line
    }
    pub fn source_index(&self, display: usize) -> usize {
        if self.mapping.is_empty() {
            display.min(self.source_len)
        } else {
            self.mapping[display.min(self.mapping.len() - 1)]
        }
    }
    pub fn display_index(&self, source: usize) -> usize {
        if self.mapping.is_empty() {
            source.min(self.text.len())
        } else {
            self.mapping
                .partition_point(|&i| i < source)
                .min(self.text.len())
        }
    }
    /// Keep projection work bounded while source rows remain independently virtualized.
    pub fn slice(&self, range: Range<usize>) -> Self {
        let start = self.display_index(range.start);
        let end = self.display_index(range.end);
        // Source chunk boundaries are UTF-8/grapheme boundaries. Hidden syntax
        // maps to the next displayed character, so these are boundaries too.
        let text = self.text[start..end].to_owned();
        let spans = self
            .spans
            .iter()
            .filter(|s| s.range.start < end && s.range.end > start)
            .map(|s| Span {
                range: s.range.start.max(start) - start..s.range.end.min(end) - start,
                style: s.style.clone(),
            })
            .collect();
        let mapping = (start..=end)
            .map(|i| self.source_index(i).clamp(range.start, range.end) - range.start)
            .collect();
        Self {
            text,
            spans,
            kind: self.kind,
            mapping,
            source_len: range.len(),
        }
    }
}

mod context;
pub use context::{Context, LiveCallout};
pub mod rows;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projections_preserve_source_positions_and_unicode() {
        for source in [
            "# Héllo **world**",
            "> *quote*",
            "- [x] **done**",
            "[[Target|é👩‍💻]] and [site](https://example.com)",
            "\\*literal\\* and `**code**`",
            "---",
            "",
        ] {
            let line = Line::new(source, false, false, false);
            assert_eq!(Line::new(source, true, false, false).text, source);
            for (i, _) in line.text.char_indices() {
                assert!(
                    source.is_char_boundary(line.source_index(i)),
                    "{source}: {i}"
                );
            }
            assert!(source.is_char_boundary(line.source_index(line.text.len())));
            assert!(line.mapping.windows(2).all(|w| w[0] <= w[1]));
        }
        let source = "[[Target|é👩‍💻]] and **bold**";
        let line = Line::new(source, false, false, false);
        assert_eq!(line.text, "é👩‍💻 and bold");
        assert_eq!(line.source_index(0), source.find('é').unwrap());
        assert_eq!(
            line.source_index(line.text.find("bold").unwrap()),
            source.find("bold").unwrap()
        );
        assert_eq!(
            Line::new("**literal**", false, true, false).text,
            "**literal**"
        );
    }
    #[test]
    fn slices_keep_formatting_across_source_chunks() {
        let source = format!("**{}** [[note|{}]]", "é".repeat(200), "z".repeat(200));
        let line = Line::new(&source, false, false, false);
        let chunks = crate::document::source_rows(&source);
        let rebuilt: String = chunks.iter().map(|r| line.slice(r.clone()).text).collect();
        assert_eq!(rebuilt, line.text);
        for range in chunks {
            let slice = line.slice(range.clone());
            assert!(slice.spans.iter().all(|s| s.range.end <= slice.text.len()));
            for (i, _) in slice.text.char_indices() {
                assert!(source.is_char_boundary(range.start + slice.source_index(i)));
            }
        }
    }
}
