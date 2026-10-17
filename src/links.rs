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
    /// Definitions are retained for move rewrites even when no link uses them.
    pub used: bool,
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
    // Metadata is not Markdown. Parsing an unclosed HTML comment from a YAML
    // literal could otherwise consume the note body beyond the closing fence.
    let body_start = hidden.first().map_or(0, |r| r.end);
    let body = &source[body_start..];
    let bytes = source.as_bytes();
    if body.contains("<!--") {
        for (event, range) in Parser::new_ext(body, options()).into_offset_iter() {
            if matches!(event, Event::Html(_) | Event::InlineHtml(_))
                && body[range.clone()].starts_with("<!--")
            {
                let start = body_start + range.start;
                let end = source[start + 4..]
                    .find("-->")
                    .map_or(source.len(), |n| start + 4 + n + 3);
                hidden.push(start..end);
            }
        }
    }
    if body.contains("%%") {
        let mut protected = Vec::new();
        for (event, range) in Parser::new_ext(body, options()).into_offset_iter() {
            if matches!(
                event,
                Event::Code(_)
                    | Event::Start(Tag::CodeBlock(_))
                    | Event::Html(_)
                    | Event::InlineHtml(_)
            ) {
                protected.push(body_start + range.start..body_start + range.end);
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
    merge_ranges(hidden)
}

fn merge_ranges(mut hidden: Vec<Range<usize>>) -> Vec<Range<usize>> {
    hidden.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in hidden {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
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
    pub(crate) fn hidden_ranges(&self) -> Vec<Range<usize>> {
        let mut previous = 0;
        self.skips
            .iter()
            .map(|&(at, skipped)| {
                let range = at + previous..at + skipped;
                previous = skipped;
                range
            })
            .collect()
    }
    pub fn original_offset(&self, offset: usize) -> usize {
        let index = self.skips.partition_point(|(at, _)| *at <= offset);
        offset + index.checked_sub(1).map_or(0, |i| self.skips[i].1)
    }
}
pub(crate) fn visible_source(source: &str) -> VisibleSource<'_> {
    visible_source_with(source, std::iter::empty())
}

/// Additional parser-recognized syntax uses the same sparse source map as
/// comments. Callers supply complete UTF-8 ranges from document analysis.
pub(crate) fn visible_source_with(
    source: &str,
    extra: impl IntoIterator<Item = Range<usize>>,
) -> VisibleSource<'_> {
    let hidden = merge_ranges(hidden_ranges(source).into_iter().chain(extra).collect());
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

mod analysis;
pub use analysis::{Analysis, analyze, references};

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
mod tests;
