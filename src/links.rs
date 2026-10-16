//! Source-preserving local references for file moves. Parsing identifies links;
//! only destination bytes are edited, leaving labels, titles and layout intact.
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub target: String,
    pub destination: Range<usize>,
    pub wiki: bool,
}

pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_WIKILINKS
}

/// Hide frontmatter and Obsidian comments without changing byte offsets. Code
/// spans/fences and escaped percent signs do not start comments.
fn hidden_ranges(source: &str) -> Vec<Range<usize>> {
    let mut hidden = Vec::new();
    if let Ok(Some(header)) = crate::properties::frontmatter(source) {
        hidden.push(header.range);
    }
    let bytes = source.as_bytes();
    if source.contains("%%") {
        let mut protected = Vec::new();
        for (event, range) in Parser::new_ext(source, options()).into_offset_iter() {
            if matches!(
                event,
                Event::Code(_)
                    | Event::Start(Tag::CodeBlock(_))
                    | Event::Html(_)
                    | Event::InlineHtml(_)
            ) {
                protected.push(range);
            }
        }
        protected.extend(hidden.iter().cloned());
        protected.sort_by_key(|r| r.start);
        let mut i = 0;
        let mut p = 0;
        while i < bytes.len() {
            while p < protected.len() && protected[p].end <= i {
                p += 1;
            }
            if p < protected.len() && protected[p].contains(&i) {
                i = protected[p].end;
            } else if bytes[i] == b'\\' {
                i = (i + 2).min(bytes.len());
            } else if bytes[i..].starts_with(b"%%") {
                let end = source[i + 2..]
                    .find("%%")
                    .map_or(source.len(), |n| i + 2 + n + 2);
                hidden.push(i..end);
                i = end;
            } else {
                i += 1;
            }
        }
    }
    hidden.sort_by_key(|r| r.start);
    hidden
}

pub fn masked_source(source: &str) -> std::borrow::Cow<'_, str> {
    let hidden = hidden_ranges(source);
    let bytes = source.as_bytes();
    if hidden.is_empty() {
        return std::borrow::Cow::Borrowed(source);
    }
    let mut masked = bytes.to_vec();
    for range in hidden {
        for byte in &mut masked[range] {
            if !matches!(*byte, b'\r' | b'\n') {
                *byte = b' ';
            }
        }
    }
    std::borrow::Cow::Owned(
        String::from_utf8(masked).expect("masking complete source ranges preserves UTF-8"),
    )
}

/// Remove hidden syntax before block parsing so a leading comment cannot turn
/// the remaining paragraph into indented code. A sparse offset map restores the
/// original source positions without allocating a byte-sized mapping array.
pub(crate) struct VisibleSource<'a> {
    pub text: std::borrow::Cow<'a, str>,
    skips: Vec<(usize, usize)>,
}
impl VisibleSource<'_> {
    pub fn original_offset(&self, offset: usize) -> usize {
        let index = self.skips.partition_point(|(at, _)| *at <= offset);
        offset + index.checked_sub(1).map_or(0, |i| self.skips[i].1)
    }
}
pub(crate) fn visible_source(source: &str) -> VisibleSource<'_> {
    let hidden = hidden_ranges(source);
    if hidden.is_empty() {
        return VisibleSource {
            text: source.into(),
            skips: vec![],
        };
    }
    let mut text = String::with_capacity(source.len());
    let mut skips = vec![];
    let mut end = 0;
    for range in hidden {
        text.push_str(&source[end..range.start]);
        end = range.end;
        skips.push((text.len(), end - text.len()));
    }
    text.push_str(&source[end..]);
    VisibleSource {
        text: text.into(),
        skips,
    }
}

fn destination(raw: &str, start: usize) -> Option<Range<usize>> {
    let bytes = raw.as_bytes();
    let mut i = start;
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    if bytes.get(i) == Some(&b'<') {
        let start = i + 1;
        i = start;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'>' {
                return Some(start..i);
            }
            i += 1;
        }
        return None;
    }
    let start = i;
    let mut depth = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'(' => depth += 1,
            b')' if depth == 0 => break,
            b')' => depth -= 1,
            b if b.is_ascii_whitespace() => break,
            _ => (),
        }
        i += 1;
    }
    (i <= bytes.len()).then_some(start..i)
}

fn label_end(raw: &str) -> Option<usize> {
    let mut depth = 0;
    let bytes = raw.as_bytes();
    let mut i = usize::from(raw.starts_with('!'));
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => (),
        }
        i += 1;
    }
    None
}

pub struct Analysis {
    pub references: Vec<Reference>,
    pub wiki_links: Vec<crate::markdown::WikiLink>,
    pub tags: Vec<String>,
}
pub fn references(source: &str) -> Vec<Reference> {
    analyze(source).references
}

/// Extract saved-note metadata in one parser pass, without retaining the body.
pub fn analyze(source: &str) -> Analysis {
    let visible = visible_source(source);
    let parser = Parser::new_ext(&visible.text, options());
    let mut references = Vec::new();
    let mut wiki_links = Vec::new();
    let mut tags = std::collections::BTreeSet::new();
    let (mut code_depth, mut link_depth): (usize, usize) = (0, 0);
    let mut previous = None;
    let base = visible.text.as_ptr() as usize;
    for (_, def) in parser.reference_definitions().iter() {
        let span = visible.original_offset(def.span.start)..visible.original_offset(def.span.end);
        let raw = &source[span.clone()];
        if let Some(end) = label_end(raw)
            && raw.as_bytes().get(end) == Some(&b':')
            && let Some(range) = destination(raw, end + 1)
        {
            references.push(Reference {
                target: def.dest.to_string(),
                destination: span.start + range.start..span.start + range.end,
                wiki: false,
            });
        }
    }
    for (event, range) in parser.into_offset_iter() {
        match &event {
            Event::Start(Tag::CodeBlock(_)) => code_depth += 1,
            Event::End(TagEnd::CodeBlock) => code_depth = code_depth.saturating_sub(1),
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => link_depth += 1,
            Event::End(TagEnd::Link | TagEnd::Image) => link_depth = link_depth.saturating_sub(1),
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::Item | Tag::TableCell)
            | Event::SoftBreak
            | Event::HardBreak => previous = Some('\n'),
            Event::Text(text) => {
                let pointer = text.as_ptr() as usize;
                if code_depth == 0
                    && link_depth == 0
                    && pointer >= base
                    && pointer + text.len() <= base + visible.text.len()
                {
                    for tag in crate::markdown::text_tags(text, previous) {
                        tags.insert(text[tag.start + 1..tag.end].to_owned());
                    }
                }
                previous = text.chars().next_back().or(previous);
            }
            Event::Code(text) => previous = text.chars().next_back().or(previous),
            _ => (),
        }
        let Event::Start(
            Tag::Link {
                link_type,
                dest_url,
                ..
            }
            | Tag::Image {
                link_type,
                dest_url,
                ..
            },
        ) = event
        else {
            continue;
        };
        if matches!(link_type, LinkType::WikiLink { .. }) {
            let raw = &visible.text[range.clone()];
            if let Some(start) = raw.find("[[")
                && let Some(inner) = raw[start + 2..].strip_suffix("]]")
            {
                let (target, label) = inner.split_once('|').unwrap_or((inner, inner));
                let target = target.trim_end_matches('\\').trim();
                if !target.is_empty() {
                    wiki_links.push(crate::markdown::WikiLink {
                        target: target.into(),
                        label: label.trim().into(),
                        range: visible.original_offset(range.start + start)
                            ..visible.original_offset(range.end),
                    });
                }
            }
        }
        let range = visible.original_offset(range.start)..visible.original_offset(range.end);
        let raw = &source[range.clone()];
        let wiki = matches!(link_type, LinkType::WikiLink { .. });
        let relative = if wiki {
            let start = raw.find("[[").map(|i| i + 2);
            start.and_then(|start| {
                let end = raw[start..].find('|').or_else(|| raw[start..].find("]]"))? + start;
                let target = raw[start..end]
                    .strip_suffix('\\')
                    .unwrap_or(&raw[start..end]);
                if target.trim().is_empty() {
                    return None;
                }
                let end = start + target.len();
                let leading = target.len() - target.trim_start().len();
                Some(start + leading..end - (target.len() - target.trim_end().len()))
            })
        } else if link_type == LinkType::Inline {
            label_end(raw)
                .filter(|&i| raw.as_bytes().get(i) == Some(&b'('))
                .and_then(|i| destination(raw, i + 1))
        } else {
            None
        };
        if let Some(relative) = relative {
            references.push(Reference {
                target: if wiki {
                    raw[relative.clone()].trim().to_owned()
                } else {
                    dest_url.to_string()
                },
                destination: range.start + relative.start..range.start + relative.end,
                wiki,
            });
        }
    }
    references.sort_by_key(|r| r.destination.start);
    references.dedup_by(|a, b| a.destination == b.destination);
    tags.extend(crate::properties::tags(source));
    Analysis {
        references,
        wiki_links,
        tags: tags.into_iter().collect(),
    }
}

pub fn decode_url(path: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(path.len());
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            ((bytes.next()? as char).to_digit(16)? * 16 + (bytes.next()? as char).to_digit(16)?)
                as u8
        } else {
            byte
        });
    }
    String::from_utf8(decoded).ok()
}

fn normalized(base: &Path, path: &str) -> Option<PathBuf> {
    if path.contains(['\\', '\0', ':']) || path.starts_with("//") {
        return None;
    }
    let mut result = if path.starts_with('/') {
        PathBuf::new()
    } else {
        base.to_owned()
    };
    for component in Path::new(path.trim_start_matches('/')).components() {
        match component {
            Component::Normal(name) => {
                if name.to_string_lossy().starts_with('.') {
                    return None;
                }
                result.push(name);
            }
            Component::CurDir => (),
            Component::ParentDir if result.pop() => (),
            _ => return None,
        }
    }
    Some(result)
}

#[derive(Clone, Debug, Default)]
pub struct Resolver {
    files: HashSet<PathBuf>,
    names: HashMap<String, Option<PathBuf>>,
}
impl Resolver {
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let files: HashSet<_> = paths.into_iter().collect();
        let mut names = HashMap::new();
        for path in &files {
            if let Some(name) = path.file_name() {
                names
                    .entry(name.to_string_lossy().into_owned())
                    .and_modify(|entry| *entry = None)
                    .or_insert_with(|| Some(path.clone()));
            }
        }
        Self { files, names }
    }
    pub fn resolve(&self, from: &Path, target: &str, wiki: bool) -> Option<PathBuf> {
        let path = target
            .split(if wiki { &['#'][..] } else { &['#', '?'][..] })
            .next()?
            .trim();
        if path.is_empty() {
            return Some(from.to_owned());
        }
        let path = if wiki {
            path.to_owned()
        } else {
            decode_url(path)?
        };
        let local = normalized(from.parent().unwrap_or(Path::new("")), &path)?;
        let root = normalized(Path::new(""), &path);
        for candidate in std::iter::once(local).chain(root.filter(|_| wiki)) {
            if self.files.contains(&candidate) {
                return Some(candidate);
            }
            let with_md = PathBuf::from(format!("{}.md", candidate.to_string_lossy()));
            if self.files.contains(&with_md) {
                return Some(with_md);
            }
        }
        if !wiki || path.starts_with('/') || path.contains('/') {
            return None;
        }
        self.names
            .get(&path)
            .or_else(|| self.names.get(&format!("{path}.md")))
            .cloned()
            .flatten()
    }
}

pub fn relative_url(from: &Path, to: &Path) -> String {
    let parent: Vec<_> = from
        .parent()
        .unwrap_or(Path::new(""))
        .components()
        .collect();
    let target: Vec<_> = to.components().collect();
    let common = parent
        .iter()
        .zip(&target)
        .take_while(|(a, b)| a == b)
        .count();
    let path = "../".repeat(parent.len() - common)
        + &target[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
}

pub fn retarget(from: &Path, to: &Path, original: &str, wiki: bool) -> Option<String> {
    let split = original
        .find(if wiki { &['#'][..] } else { &['#', '?'][..] })
        .unwrap_or(original.len());
    let suffix = &original[split..];
    if wiki {
        let path = if !original[..split].ends_with(".md")
            && to.extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            to.with_extension("")
        } else {
            to.to_owned()
        };
        let path = path.to_string_lossy().replace('\\', "/");
        if path.contains(['#', '|']) || path.contains("]]") {
            return None;
        }
        Some(format!("/{path}{suffix}"))
    } else {
        Some(format!("{}{suffix}", relative_url(from, to)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn single_pass_tags_match_reading_and_exclude_links_comments_and_code() {
        let source = "---\ntags: [yaml]\n---\n**#bold** #plain `#code` [#label](Target.md) ![#alt](Image.png)\n\n| First | #cell |\n| --- | --- |\n| Text | #next |\n\n    #indented\n\n%%#hidden%%";
        let analysis = analyze(source);
        assert_eq!(analysis.tags, ["bold", "cell", "next", "plain", "yaml"]);
        assert_eq!(analysis.tags, crate::prepared::Prepared::new(source).tags);
        assert_eq!(analysis.references.len(), 2);
    }
    #[test]
    fn empty_and_incomplete_wiki_links_never_create_invalid_ranges() {
        for source in [
            "[[ ]]",
            "[[\t|alias]]",
            "[[　]]",
            "[[|alias]]",
            "![[ ]]",
            "[[",
            "[[Target",
            "%%hidden%%[[ ]] [[Real]]",
        ] {
            for reference in references(source) {
                assert!(!reference.target.is_empty());
                assert!(source.get(reference.destination).is_some());
            }
        }
    }
    #[test]
    fn references_preserve_exact_destination_ranges_and_ignore_literal_text() {
        let source = "---\nlink: '[[YAML]]'\n---\n[[Target#Heading|Alias]] ![[Picture.png|120]]\n[**label**](../A%20note.md#Part \"title\")\n![alt](<images/a (b).png>)\n[ref][r]\n\n[r]: <../Folder/File.md> 'a title'\n\n`[[code]]` `%%` [[Visible]]\n%% [[hidden]] [hidden](Hide.md) %%\n```\n[[fenced]]\n```\n";
        let refs = references(source);
        assert_eq!(
            refs.iter()
                .map(|r| &source[r.destination.clone()])
                .collect::<Vec<_>>(),
            [
                "Target#Heading",
                "Picture.png",
                "../A%20note.md#Part",
                "images/a (b).png",
                "../Folder/File.md",
                "Visible"
            ]
        );
    }
    #[test]
    fn relative_urls_handle_unicode_reserved_characters_and_parent_segments() {
        let files =
            Resolver::new(["A/One.md", "Images/é (x)#1.png", "A/Target.md"].map(PathBuf::from));
        let from = Path::new("A/One.md");
        let target = Path::new("Images/é (x)#1.png");
        let url = relative_url(from, target);
        assert_eq!(url, "../Images/%C3%A9%20%28x%29%231.png");
        assert_eq!(files.resolve(from, &url, false).as_deref(), Some(target));
        assert_eq!(
            files.resolve(from, "Target#Part", true),
            Some("A/Target.md".into())
        );
        assert_eq!(files.resolve(from, "../../../outside.png", false), None);
        assert_eq!(
            files.resolve(from, "https://example.com/file.md", false),
            None
        );
        assert_eq!(
            retarget(from, Path::new("Else/Target.md"), "Target#Part", true).unwrap(),
            "/Else/Target#Part"
        );
    }
}
