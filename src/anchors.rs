//! Compact heading/block targets, extracted alongside saved-note links.
use std::{collections::BTreeMap, ops::Range};

#[derive(Clone, Debug)]
pub struct Heading {
    pub title: String,
    pub level: u8,
    pub source: Range<usize>,
    /// Includes the heading and its subsections, stopping at a peer or ancestor.
    pub section: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub source: Range<usize>,
    pub marker: Range<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Index {
    pub headings: Vec<Heading>,
    pub blocks: BTreeMap<String, Block>,
    /// Every recognized marker, including duplicate IDs whose first block wins.
    pub markers: Vec<Range<usize>>,
    heading_names: BTreeMap<String, Vec<usize>>,
}

impl Index {
    pub fn parse(source: &str) -> Self {
        let visible = crate::links::visible_source(source);
        let mut builder = Builder::default();
        for (event, range) in
            pulldown_cmark::Parser::new_ext(&visible.text, crate::links::options())
                .into_offset_iter()
        {
            builder.event(&event, range, &visible.text);
        }
        builder.finish(&visible, source.len())
    }

    /// An empty fragment targets the start; hierarchical headings are scoped to
    /// their parent's section. Duplicate headings/IDs use the first occurrence.
    pub fn target(&self, fragment: &str) -> Option<Range<usize>> {
        let fragment = crate::links::decode_url(fragment).unwrap_or_else(|| fragment.to_owned());
        let fragment = fragment.trim();
        if fragment.is_empty() {
            return Some(0..0);
        }
        if let Some(id) = fragment.strip_prefix('^') {
            return self.blocks.get(id).map(|b| b.source.clone());
        }
        let mut parent: Option<&Heading> = None;
        for part in fragment.split('#') {
            if part.trim().is_empty() {
                return None;
            }
            parent = Some(
                self.heading_names
                    .get(&normalized(part))?
                    .iter()
                    .map(|&i| &self.headings[i])
                    .find(|heading| {
                        parent.is_none_or(|p| {
                            heading.level > p.level
                                && heading.source.start > p.source.start
                                && heading.source.start < p.section.end
                        })
                    })?,
            );
        }
        parent.map(|h| h.section.clone())
    }
}

pub fn heading_matches(title: &str, anchor: &str) -> bool {
    let title = normalized(title);
    let anchor = normalized(anchor);
    title == anchor || slug(&title) == anchor
}

fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn slug(title: &str) -> String {
    title
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
}

mod builder;
pub(crate) use builder::Builder;

#[cfg(test)]
mod tests;
