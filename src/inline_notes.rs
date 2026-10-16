//! Route Obsidian inline footnote contents into their own inline builders.
//! Original parser events retain reference-link resolution and source offsets.
use crate::{inline::Builder, links};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

pub(crate) struct Note {
    pub range: Range<usize>,
    pub body: Range<usize>,
    pub label: String,
    pub inline: Builder,
    pub emitted: bool,
}
pub(crate) fn discover(source: &str) -> Vec<Note> {
    if !source.contains("^[") {
        return vec![];
    }
    let mut protected = vec![];
    let mut labels = HashSet::new();
    for (event, range) in Parser::new_ext(source, links::options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { .. })
                if range.start > 0 && source.as_bytes()[range.start - 1] == b'^' => {}
            Event::Start(Tag::CodeBlock(_) | Tag::Image { .. } | Tag::Link { .. })
            | Event::Code(_)
            | Event::Html(_)
            | Event::InlineHtml(_) => protected.push(range),
            Event::Start(Tag::FootnoteDefinition(label)) => {
                labels.insert(label.into_string());
            }
            _ => (),
        }
    }
    protected.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = vec![];
    for range in protected {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    let closing = closing_brackets(source, &merged);
    let mut notes = vec![];
    let mut consumed = 0;
    for (start, _) in source.match_indices("^[") {
        if start < consumed
            || source[..start]
                .bytes()
                .rev()
                .take_while(|b| *b == b'\\')
                .count()
                % 2
                == 1
        {
            continue;
        }
        let Some(&end) = closing.get(&(start + 1)) else {
            continue;
        };
        let mut label = format!("\0opalglass-inline-{start}");
        while labels.contains(&label) {
            label.push('_');
        }
        labels.insert(label.clone());
        consumed = end + 1;
        notes.push(Note {
            range: start..end + 1,
            body: start + 2..end,
            label,
            inline: Builder::new(false),
            emitted: false,
        });
    }
    notes
}
fn closing_brackets(source: &str, protected: &[Range<usize>]) -> HashMap<usize, usize> {
    let mut closed = HashMap::new();
    let mut stack = vec![];
    let (mut at, mut skip) = (0, 0);
    while at < source.len() {
        while skip < protected.len() && protected[skip].end <= at {
            skip += 1;
        }
        if let Some(range) = protected.get(skip).filter(|r| r.contains(&at)) {
            at = range.end;
            continue;
        }
        match source.as_bytes()[at] {
            b'\\' => {
                at += 1;
                at += source[at..].chars().next().map_or(0, char::len_utf8);
                continue;
            }
            b'[' => stack.push(at),
            b']' => {
                if let Some(start) = stack.pop()
                    && start > 0
                    && source.as_bytes()[start - 1] == b'^'
                {
                    closed.insert(start, at);
                }
            }
            b'\n'
                if source[at + 1..]
                    .trim_start_matches([' ', '\t', '\r'])
                    .starts_with('\n') =>
            {
                stack.clear()
            }
            _ => (),
        }
        at += 1;
    }
    closed
}

pub(crate) fn route<'a>(
    notes: &mut [Note],
    event: Event<'a>,
    range: Range<usize>,
    source: &'a str,
    mut emit: impl FnMut(Event<'a>, Range<usize>),
) {
    // Container ranges cover whole blocks; those stay in the main document.
    let inline = matches!(
        event,
        Event::Text(_)
            | Event::Code(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Start(
                Tag::Emphasis
                    | Tag::Strong
                    | Tag::Strikethrough
                    | Tag::Link { .. }
                    | Tag::Image { .. }
            )
            | Event::End(
                TagEnd::Emphasis
                    | TagEnd::Strong
                    | TagEnd::Strikethrough
                    | TagEnd::Link
                    | TagEnd::Image
            )
    );
    if !inline {
        emit(event, range);
        return;
    }
    let start = notes.partition_point(|n| n.range.end <= range.start);
    let end = notes.partition_point(|n| n.range.start < range.end);
    if start >= end {
        emit(event, range);
        return;
    }
    if let Event::Text(value) = &event {
        let base = source.as_ptr() as usize;
        let ptr = value.as_ptr() as usize;
        // Plain text can straddle a delimiter. Decoded entity events are wholly
        // inside or outside a body and must retain their decoded value.
        if ptr >= base && ptr + value.len() <= base + source.len() {
            let actual = ptr - base..ptr - base + value.len();
            let mut cursor = actual.start;
            for note in &mut notes[start..end] {
                if cursor < note.range.start {
                    let to = note.range.start.min(actual.end);
                    emit(Event::Text(source[cursor..to].into()), cursor..to);
                }
                if actual.contains(&note.range.start) && !note.emitted {
                    emit(
                        Event::FootnoteReference(note.label.clone().into()),
                        note.range.clone(),
                    );
                    note.emitted = true;
                }
                let body = actual.start.max(note.body.start)..actual.end.min(note.body.end);
                if body.start < body.end {
                    note.inline
                        .event(Event::Text(source[body.clone()].into()), body, source, 0);
                }
                cursor = actual.end.min(note.range.end);
            }
            if cursor < actual.end {
                emit(
                    Event::Text(source[cursor..actual.end].into()),
                    cursor..actual.end,
                );
            }
            return;
        }
    }
    if let Some(note) = notes[start..end]
        .iter_mut()
        .find(|n| n.range.start <= range.start && range.end <= n.range.end)
    {
        if note.body.start <= range.start && range.end <= note.body.end {
            note.inline.event(event, range, source, 0);
        }
    } else {
        emit(event, range);
    }
}
