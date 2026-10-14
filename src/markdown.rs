//! A deliberately small, original Markdown parser for the native preview.
//! No HTML evaluation, scripting, or source from Zed's editor.
use std::ops::Range;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: InlineStyle,
}

/// Parse common inline formatting into display text and non-overlapping UTF-8
/// ranges. Unsupported syntax remains literal. This is not a CommonMark parser.
pub fn inline(source: &str) -> (String, Vec<Span>) {
    fn parse(
        source: &str,
        inherited: InlineStyle,
        depth: usize,
        out: &mut String,
        spans: &mut Vec<Span>,
    ) {
        fn append(text: &str, style: &InlineStyle, out: &mut String, spans: &mut Vec<Span>) {
            let start = out.len();
            out.push_str(text);
            if start != out.len() && *style != InlineStyle::default() {
                if let Some(previous) = spans.last_mut()
                    && previous.range.end == start
                    && previous.style == *style
                {
                    previous.range.end = out.len();
                    return;
                }
                spans.push(Span {
                    range: start..out.len(),
                    style: style.clone(),
                });
            }
        }
        if depth > 16 {
            append(source, &inherited, out, spans);
            return;
        }
        let mut rest = source;
        let mut missing = [false; 6];
        let wiki_end = source.rfind("]]");
        let link_middle = source.rfind("](");
        let link_end = source.rfind(')');
        while !rest.is_empty() {
            if let Some(escaped) = rest.strip_prefix('\\')
                && let Some(c) = escaped.chars().next()
                && c.is_ascii_punctuation()
            {
                append(&escaped[..c.len_utf8()], &inherited, out, spans);
                rest = &escaped[c.len_utf8()..];
                continue;
            }
            if rest.starts_with("[[")
                && wiki_end.is_some_and(|end| end >= source.len() - rest.len() + 2)
                && let Some(end) = rest[2..].find("]]")
            {
                let inner = &rest[2..end + 2];
                let (target, label) = inner.split_once('|').unwrap_or((inner, inner));
                let mut style = inherited.clone();
                style.target = Some(target.trim().into());
                append(label.trim(), &style, out, spans);
                rest = &rest[end + 4..];
                continue;
            }
            if rest.starts_with('[')
                && !rest.starts_with("[[")
                && link_end.is_some_and(|end| end > source.len() - rest.len())
                && link_middle.is_some_and(|end| end > source.len() - rest.len())
                && let Some(middle) = rest.find("](")
                && let Some(end) = rest[middle + 2..].find(')')
            {
                let target = &rest[middle + 2..middle + 2 + end];
                let mut style = inherited.clone();
                style.target = Some(target.into());
                append(&rest[1..middle], &style, out, spans);
                rest = &rest[middle + end + 3..];
                continue;
            }
            let mut matched = false;
            for (index, delimiter) in ["`", "**", "__", "~~", "*", "_"].into_iter().enumerate() {
                if missing[index] {
                    continue;
                }
                if let Some(tail) = rest.strip_prefix(delimiter)
                    && !tail.contains(delimiter)
                {
                    missing[index] = true;
                    continue;
                }
                if let Some(tail) = rest.strip_prefix(delimiter)
                    && let Some(end) = tail.find(delimiter)
                    && end > 0
                {
                    let mut style = inherited.clone();
                    match delimiter {
                        "`" => style.code = true,
                        "**" | "__" => style.bold = true,
                        "~~" => style.strike = true,
                        _ => style.italic = true,
                    }
                    if style.code {
                        append(&tail[..end], &style, out, spans);
                    } else {
                        parse(&tail[..end], style, depth + 1, out, spans);
                    }
                    rest = &tail[end + delimiter.len()..];
                    matched = true;
                    break;
                }
            }
            if !matched {
                let first = rest.chars().next().unwrap().len_utf8();
                let n = first
                    + rest[first..]
                        .find(['\\', '[', '`', '*', '_', '~'])
                        .unwrap_or(rest.len() - first);
                append(&rest[..n], &inherited, out, spans);
                rest = &rest[n..];
            }
        }
    }
    let mut text = String::new();
    let mut spans = Vec::new();
    parse(source, InlineStyle::default(), 0, &mut text, &mut spans);
    (text, spans)
}

pub fn blocks(source: &str) -> Vec<Block> {
    let mut result = Vec::new();
    let mut paragraph = String::new();
    let mut code = String::new();
    let mut fence: Option<(char, usize)> = None;
    fn flush(result: &mut Vec<Block>, text: &mut String) {
        if !text.is_empty() {
            result.push(Block::Paragraph(std::mem::take(text)));
        }
    }
    for line in source.lines() {
        let trimmed = line.trim();
        let marker = trimmed.chars().next().unwrap_or(' ');
        let count = trimmed.chars().take_while(|c| *c == marker).count();
        if let Some((c, n)) = fence {
            if marker == c && count >= n && trimmed[count..].trim().is_empty() {
                result.push(Block::Code(std::mem::take(&mut code)));
                fence = None;
            } else {
                code.push_str(line);
                code.push('\n');
            }
            continue;
        }
        if (marker == '`' || marker == '~') && count >= 3 {
            flush(&mut result, &mut paragraph);
            fence = Some((marker, count));
            continue;
        }
        let special = if trimmed.is_empty() {
            flush(&mut result, &mut paragraph);
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
        } else if let Some(text) = trimmed.strip_prefix("- [ ] ") {
            Some(Block::Task(false, text.into()))
        } else if let Some(text) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            Some(Block::Task(true, text.into()))
        } else {
            trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .or_else(|| trimmed.strip_prefix("+ "))
                .map(|text| Block::Item(text.into()))
        };
        if let Some(block) = special {
            flush(&mut result, &mut paragraph);
            result.push(block);
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(trimmed);
        }
    }
    flush(&mut result, &mut paragraph);
    if fence.is_some() {
        result.push(Block::Code(code));
    }
    result
}

/// Find wiki links, ignoring escaped delimiters, inline code and fenced code.
pub fn wiki_links(source: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(char, usize)> = None;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().unwrap_or(' ');
        let count = trimmed.chars().take_while(|c| *c == marker).count();
        if let Some((c, n)) = fence {
            if marker == c && count >= n && trimmed[count..].trim().is_empty() {
                fence = None;
            }
            offset += line.len();
            continue;
        }
        if (marker == '`' || marker == '~') && count >= 3 {
            fence = Some((marker, count));
            offset += line.len();
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        let mut ticks = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if bytes[i] == b'`' {
                let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
                if ticks == 0 {
                    ticks = run;
                } else if ticks == run {
                    ticks = 0;
                }
                i += run;
                continue;
            }
            if ticks == 0 && bytes[i..].starts_with(b"[[") {
                let Some(end) = line[i + 2..].find("]]") else {
                    break;
                };
                let end = i + 2 + end;
                let inner = &line[i + 2..end];
                let (target, label) = inner.split_once('|').unwrap_or((inner, inner));
                if !target.trim().is_empty() {
                    links.push(WikiLink {
                        target: target.trim().into(),
                        label: label.trim().into(),
                        range: offset + i..offset + end + 2,
                    });
                }
                i = end + 2;
                continue;
            }
            i += 1;
        }
        offset += line.len();
    }
    links
}

pub fn tags(source: &str) -> Vec<String> {
    let mut tags = std::collections::BTreeSet::new();
    for block in blocks(source) {
        let text = match block {
            Block::Heading(_, s)
            | Block::Paragraph(s)
            | Block::Quote(s)
            | Block::Item(s)
            | Block::Task(_, s) => s,
            Block::Code(_) | Block::Rule => continue,
        };
        let (display, spans) = inline(&text);
        for (index, _) in display.match_indices('#') {
            if index > 0
                && !display[..index].ends_with(|c: char| c.is_whitespace() || "([{".contains(c))
            {
                continue;
            }
            if spans
                .iter()
                .any(|s| s.range.contains(&index) && (s.style.code || s.style.target.is_some()))
            {
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
    tags.into_iter().collect()
}

pub fn heading_matches(title: &str, anchor: &str) -> bool {
    title.to_lowercase() == anchor.to_lowercase()
        || title
            .chars()
            .filter_map(|c| {
                if c.is_whitespace() {
                    Some('-')
                } else if c.is_alphanumeric() || c == '-' || c == '_' {
                    Some(c)
                } else {
                    None
                }
            })
            .collect::<String>()
            .to_lowercase()
            == anchor.to_lowercase()
}

pub fn heading_offset(source: &str, anchor: &str) -> Option<usize> {
    let mut offset = 0;
    let mut fence = false;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
        }
        let count = trimmed.chars().take_while(|c| *c == '#').count();
        if !fence
            && (1..=6).contains(&count)
            && trimmed.as_bytes().get(count) == Some(&b' ')
            && heading_matches(trimmed[count + 1..].trim_end_matches('#').trim(), anchor)
        {
            return Some(offset);
        }
        offset += line.len();
    }
    None
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
