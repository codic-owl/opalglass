use super::*;

pub struct Analysis {
    pub hidden: Vec<Range<usize>>,
    pub definitions: Vec<Range<usize>>,
    pub anchors: crate::anchors::Index,
    pub references: Vec<Reference>,
    pub wiki_links: Vec<crate::markdown::WikiLink>,
    /// Actual inline links, including used reference definitions, for editor styling.
    pub inline_links: Vec<crate::markdown::WikiLink>,
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
    let mut anchors = crate::anchors::Builder::default();
    let mut used_definitions = HashSet::new();
    let mut wiki_links = Vec::new();
    let mut inline_links = Vec::new();
    let mut definitions = Vec::new();
    let mut tags = std::collections::BTreeSet::new();
    let (mut code_depth, mut link_depth): (usize, usize) = (0, 0);
    let mut previous = None;
    let base = visible.text.as_ptr() as usize;
    for (_, def) in parser.reference_definitions().iter() {
        let span = visible.original_offset(def.span.start)..visible.original_offset(def.span.end);
        definitions.push(span.clone());
        let raw = &source[span.clone()];
        if let Some(end) = label_end(raw)
            && raw.as_bytes().get(end) == Some(&b':')
            && let Some(range) = destination(raw, end + 1)
        {
            references.push(Reference {
                target: def.dest.to_string(),
                destination: span.start + range.start..span.start + range.end,
                wiki: false,
                used: false,
            });
        }
    }
    for (event, range) in parser.into_offset_iter() {
        anchors.event(&event, range.clone(), &visible.text);
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
        if matches!(
            link_type,
            LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut
        ) {
            used_definitions.insert(dest_url.to_string());
        }
        inline_links.push(crate::markdown::WikiLink {
            target: dest_url.to_string(),
            label: String::new(),
            range: visible.original_offset(range.start)..visible.original_offset(range.end),
        });
        if matches!(link_type, LinkType::WikiLink { .. }) {
            let raw = &visible.text[range.clone()];
            if let Some(start) = raw.find("[[")
                && let Some(inner) = raw[start + 2..].strip_suffix("]]")
            {
                let (target, label) = inner.split_once('|').unwrap_or((inner, inner));
                let target = target.trim_end_matches('\\').trim();
                if !target.is_empty() {
                    inline_links.last_mut().unwrap().target = target.to_owned();
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
                used: true,
            });
        }
    }
    for reference in &mut references {
        reference.used |= used_definitions.contains(&reference.target);
    }
    references.sort_by_key(|r| r.destination.start);
    references.dedup_by(|a, b| a.destination == b.destination);
    tags.extend(crate::properties::tags(source));
    Analysis {
        hidden: visible.hidden_ranges(),
        definitions,
        anchors: anchors.finish(&visible, source.len()),
        references,
        wiki_links,
        inline_links,
        tags: tags.into_iter().collect(),
    }
}
