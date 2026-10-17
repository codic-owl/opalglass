//! Markdown metadata and the shared CommonMark inline projection.
//! Native Reading uses the whole-document parser in `rendering`.
use std::ops::Range;

pub const TAG_TARGET_PREFIX: &str = "opalglass-tag:";

pub fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_alphanumeric() || "_-/".contains(c))
        && !tag.chars().all(|c| c.is_numeric())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikiLink {
    pub target: String,
    pub label: String,
    pub range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(u8, String),
    Paragraph(String),
    Quote(String),
    Item(String),
    Task(bool, String),
    Code(String),
    Rule,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub target: Option<String>,
    pub image: bool,
    /// Parser-local source identity keeps adjacent image labels separate.
    pub image_start: usize,
    pub image_dimensions: Option<crate::images::Dimensions>,
    pub highlight: Option<HighlightColor>,
    pub syntax_color: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightColor {
    Default,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
}
impl HighlightColor {
    pub(crate) fn prefix(text: &str) -> Option<(Self, usize)> {
        let ch = text.chars().next()?;
        let color = match ch {
            '🔴' => Self::Red,
            '🟠' => Self::Orange,
            '🟡' => Self::Yellow,
            '🟢' => Self::Green,
            '🔵' => Self::Blue,
            '🟣' => Self::Purple,
            _ => return None,
        };
        Some((color, ch.len_utf8()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: InlineStyle,
}

/// Parse common inline formatting into display text and non-overlapping UTF-8
/// ranges. Obsidian-specific extensions are tracked in the coverage inventory.
pub fn inline(source: &str) -> (String, Vec<Span>) {
    let (text, spans, _) = inline_impl(source, false);
    (text, spans)
}

/// Display bytes mapped back to source bytes for direct editing of rendered text.
/// The extra final entry maps the caret after the last displayed character.
pub fn inline_mapped(source: &str) -> (String, Vec<Span>, Vec<usize>) {
    inline_impl(source, true)
}

fn inline_impl(source: &str, mapped: bool) -> (String, Vec<Span>, Vec<usize>) {
    crate::inline::parse(source, mapped)
}

pub fn blocks(source: &str) -> Vec<Block> {
    blocks_with_ranges(source)
        .into_iter()
        .map(|(block, _)| block)
        .collect()
}

/// Blocks paired with their original byte ranges, including Markdown markers.
/// Retaining these during parsing lets the UI synchronize virtualized panes.
pub fn blocks_with_ranges(source: &str) -> Vec<(Block, Range<usize>)> {
    let mut result = Vec::new();
    let mut paragraph = String::new();
    let mut code = String::new();
    let mut fence: Option<(char, usize)> = None;
    let mut paragraph_range = 0..0;
    let mut fence_start = 0;
    let mut offset = crate::properties::frontmatter(source)
        .ok()
        .flatten()
        .map_or(0, |h| h.range.end);
    let content = &source[offset..];
    fn flush(result: &mut Vec<(Block, Range<usize>)>, text: &mut String, range: &Range<usize>) {
        if !text.is_empty() {
            result.push((Block::Paragraph(std::mem::take(text)), range.clone()));
        }
    }
    for raw in content.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let line = raw
            .strip_suffix("\r\n")
            .or_else(|| raw.strip_suffix('\n'))
            .unwrap_or(raw);
        let trimmed = line.trim();
        let marker = trimmed.chars().next().unwrap_or(' ');
        let count = trimmed.chars().take_while(|c| *c == marker).count();
        if let Some((c, n)) = fence {
            if marker == c && count >= n && trimmed[count..].trim().is_empty() {
                result.push((Block::Code(std::mem::take(&mut code)), fence_start..offset));
                fence = None;
            } else {
                code.push_str(line);
                code.push('\n');
            }
            continue;
        }
        if (marker == '`' || marker == '~') && count >= 3 {
            flush(&mut result, &mut paragraph, &paragraph_range);
            fence_start = start;
            fence = Some((marker, count));
            continue;
        }
        let special = if trimmed.is_empty() {
            flush(&mut result, &mut paragraph, &paragraph_range);
            continue;
        } else if marker == '#' && count <= 6 && trimmed.as_bytes().get(count) == Some(&b' ') {
            Some(Block::Heading(
                count as u8,
                trimmed[count + 1..].trim_end_matches('#').trim().into(),
            ))
        } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            Some(Block::Rule)
        } else if let Some(text) = trimmed.strip_prefix("> ") {
            Some(Block::Quote(text.into()))
        } else if let Some(prefix) = crate::editing::list_prefix(trimmed)
            && let Some(done) = prefix.task
        {
            Some(Block::Task(done, trimmed[prefix.end..].into()))
        } else {
            trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .or_else(|| trimmed.strip_prefix("+ "))
                .map(|text| Block::Item(text.into()))
        };
        if let Some(block) = special {
            flush(&mut result, &mut paragraph, &paragraph_range);
            result.push((block, start..offset));
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            } else {
                paragraph_range.start = start;
            }
            paragraph_range.end = offset;
            paragraph.push_str(trimmed);
        }
    }
    flush(&mut result, &mut paragraph, &paragraph_range);
    if fence.is_some() {
        result.push((Block::Code(code), fence_start..source.len()));
    }
    result
}

/// Find wiki links, ignoring escaped delimiters, inline code and fenced code.
pub fn wiki_links(source: &str) -> Vec<WikiLink> {
    crate::links::analyze(source).wiki_links
}

pub fn tags(source: &str) -> Vec<String> {
    crate::links::analyze(source).tags
}

pub(crate) fn text_tags(text: &str, previous: Option<char>) -> Vec<Range<usize>> {
    let mut ranges = vec![];
    for (index, _) in text.match_indices('#') {
        let before = text[..index].chars().next_back().or(previous);
        let highlighted = text[..index].ends_with("==")
            || ["🔴", "🟠", "🟡", "🟢", "🔵", "🟣"].iter().any(|color| {
                text[..index]
                    .strip_suffix(color)
                    .is_some_and(|prefix| prefix.ends_with("=="))
            });
        if !highlighted && before.is_some_and(|c| !c.is_whitespace() && !"([{ ".contains(c)) {
            continue;
        }
        let end = text[index + 1..]
            .find(|c: char| !c.is_alphanumeric() && !"_-/".contains(c))
            .map_or(text.len(), |n| index + 1 + n);
        if valid_tag(&text[index + 1..end]) {
            ranges.push(index..end);
        }
    }
    ranges
}

pub use crate::anchors::heading_matches;

pub fn heading_offset(source: &str, anchor: &str) -> Option<usize> {
    crate::anchors::Index::parse(source)
        .target(anchor)
        .map(|r| r.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_handle_aliases_unicode_and_code() {
        let source =
            "[[İstanbul#Food|Lunch]] `[[no]]` \\[[escaped]]\n```md\n[[hidden]]\n```\n[[Yes]]";
        let links = wiki_links(source);
        assert_eq!(
            links.iter().map(|l| l.target.as_str()).collect::<Vec<_>>(),
            ["İstanbul#Food", "Yes"]
        );
        assert_eq!(&source[links[0].range.clone()], "[[İstanbul#Food|Lunch]]");
        assert_eq!(links[0].label, "Lunch");
    }
    #[test]
    fn preview_blocks_preserve_code() {
        assert_eq!(
            blocks("# Title\n\nA paragraph\ncontinues\n\n- [x] Done\n```rust\n  let x = 1;\n```"),
            vec![
                Block::Heading(1, "Title".into()),
                Block::Paragraph("A paragraph continues".into()),
                Block::Task(true, "Done".into()),
                Block::Code("  let x = 1;\n".into())
            ]
        );
    }

    #[test]
    fn inline_formatting_retains_text_links_and_unicode_offsets() {
        let (text, spans) =
            inline("A **bold _thought_** with `[[literal]]` and [[İstanbul|şehir]].");
        assert_eq!(text, "A bold thought with [[literal]] and şehir.");
        assert!(spans.iter().any(|s| s.style.bold && s.style.italic));
        let link = spans.iter().find(|s| s.style.target.is_some()).unwrap();
        assert_eq!(&text[link.range.clone()], "şehir");
        assert_eq!(link.style.target.as_deref(), Some("İstanbul"));
        assert!(spans.windows(2).all(|s| s[0].range.end <= s[1].range.start));
        assert_eq!(
            inline("[web](https://example.com) and \\*literal\\*").0,
            "web and *literal*"
        );
    }

    #[test]
    fn tags_ignore_code_headings_links_and_numbers() {
        assert_eq!(
            tags(
                "# Heading\n\n#ideas #ideas/notes (#Türkçe) #123 `#ignored` [[#anchor]]\n\n```\n#hidden\n```"
            ),
            vec!["Türkçe", "ideas", "ideas/notes"]
        );
    }

    #[test]
    fn heading_navigation_skips_mentions_and_code() {
        let source =
            "See the Deep thought heading.\n```\n# Deep thought\n```\n## Deep thought\nContent";
        assert_eq!(
            heading_offset(source, "deep-thought"),
            source.find("## Deep thought")
        );
        assert_eq!(
            heading_offset(source, "Deep thought"),
            source.find("## Deep thought")
        );
        assert_eq!(heading_offset(source, "Missing"), None);
    }
}
